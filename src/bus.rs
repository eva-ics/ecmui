use crate::common::{
    ConnectionOptions, ItemConfig, ItemState, Nit, NitData, NitKind, NodeInfo, PayloadLvarSet,
    ServiceParams,
};
use crate::ui::{self, set_status, StatusKind};
use eva_client::{EvaClient, EvaCloudClient, NodeMap};
use eva_common::common_payloads::{ParamsId, ParamsUuid};
use eva_common::prelude::*;
use lazy_static::lazy_static;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::sync::{mpsc as mpsc_std, Arc};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

lazy_static! {
    static ref WATCHERS: std::sync::Mutex<HashMap<uuid::Uuid, JoinHandle<()>>> = <_>::default();
}

const SVC_CORE: &str = "eva.core";

async fn item_watcher(
    client: Arc<EvaCloudClient>,
    u: uuid::Uuid,
    node: &str,
    oid: &OID,
    int: Duration,
) {
    match to_value(ParamsId { i: oid.as_str() }) {
        Ok(payload) => {
            let mut interval = tokio::time::interval(int);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if let Ok(value) = client
                    .call::<Value>(node, SVC_CORE, "item.state", Some(payload.clone()))
                    .await
                {
                    ui::command(ui::Command::ProcessItemWatch(u, value));
                }
            }
        }
        Err(e) => {
            eprintln!("{}", e);
        }
    }
}

async fn action_watcher(
    client: Arc<EvaCloudClient>,
    u: uuid::Uuid,
    node: &str,
    action_uuid: uuid::Uuid,
    int: Duration,
) {
    match to_value(ParamsUuid { u: action_uuid }) {
        Ok(payload) => {
            let mut interval = tokio::time::interval(int);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                match client
                    .call::<Value>(node, SVC_CORE, "action.result", Some(payload.clone()))
                    .await
                {
                    Ok(value) => ui::command(ui::Command::ProcessActionWatch(u, value)),
                    Err(e) if e.kind() == ErrorKind::ResourceNotFound => {
                        ui::command(ui::Command::ProcessActionWatch(u, Value::Unit));
                    }
                    Err(e) => {
                        eprintln!("{}", e);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("{}", e);
        }
    }
}

async fn launch_connection(
    path: &str,
    _tx: oneshot::Sender<()>,
    timeout: Duration,
    creds: Option<(String, String)>,
) {
    crate::CLIENT_CHANNEL.lock().unwrap().take();
    set_status(format!("Connecting to {path}..."), StatusKind::Info);
    macro_rules! set_err {
        ($e: expr) => {
            set_status(
                $e.message().unwrap_or("Connection error"),
                StatusKind::Error,
            );
        };
    }
    let mut client_config = eva_client::Config::new().timeout(timeout);
    if let Some(c) = creds {
        client_config = client_config.credentials(&c.0, &c.1);
    }
    let client = match EvaClient::connect(path, crate::BUS_CLIENT_NAME, client_config).await {
        Ok(c) => c,
        Err(e) => {
            set_err!(e);
            return;
        }
    };
    set_status(format!("Loading data from {path}..."), StatusKind::Info);
    crate::CLIENT_NAME
        .lock()
        .unwrap()
        .replace(client.name().to_owned());
    let sys_info: eva_client::SystemInfo = match client.call(SVC_CORE, "test", None).await {
        Ok(v) => v,
        Err(e) => {
            set_err!(e);
            return;
        }
    };
    let system_name = sys_info.system_name;
    let mut node_list: Vec<NodeInfo> = match client.call(SVC_CORE, "node.list", None).await {
        Ok(v) => v,
        Err(e) => {
            set_err!(e);
            return;
        }
    };
    node_list.sort();
    ui::command(ui::Command::MarkConnected(
        path.to_owned(),
        node_list.clone(),
    ));
    //draw_node_tree(&node_list);
    //set_status(&format!("Connected: {path}"), StatusKind::Okay);
    let mut int = tokio::time::interval(timeout / 2);
    int.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut node_map = NodeMap::new();
    for node in node_list {
        if let Some(svc) = node.svc {
            node_map.insert(node.name, svc);
        }
    }
    let cloud_client = Arc::new(EvaCloudClient::new(&system_name, client, node_map));
    let (tx, rx) = mpsc::channel(1);
    crate::CLIENT_CHANNEL.lock().unwrap().replace(tx);
    let cloud_client_c = cloud_client.clone();
    let fut = tokio::spawn(async move {
        process_commands(cloud_client_c, rx).await;
    });
    crate::NIT_HANDLER.lock().unwrap().replace(fut);
    let n = Arc::new(NitData::new_state(&system_name));
    ui::command(ui::Command::ProcessNit(n));
    loop {
        if let Err(e) = cloud_client.get_system_info(&system_name).await {
            set_err!(e);
            return;
        };
        int.tick().await;
    }
}

async fn process_bulk(
    client: &EvaCloudClient,
    node: &str,
    target: &str,
    method: &str,
    items: &[String],
) -> EResult<Value> {
    for item in items {
        if let Err(e) = client
            .call::<Value>(node, target, method, Some(to_value(ParamsId { i: item })?))
            .await
        {
            return Err(Error::failed(format!("{}: {}", item, e)));
        }
    }
    Ok(Value::Unit)
}

async fn process_lvar_set(
    client: &EvaCloudClient,
    node: &str,
    lvars: &[String],
    p_set: &PayloadLvarSet,
) -> EResult<Value> {
    for lvar in lvars {
        let mut payload = p_set.clone();
        payload.i.replace(lvar.clone());
        if let Err(e) = client
            .call::<Value>(node, SVC_CORE, "lvar.set", Some(to_value(payload)?))
            .await
        {
            return Err(Error::failed(format!("{}: {}", lvar, e)));
        }
    }
    Ok(Value::Unit)
}

async fn process_svcs(
    client: &EvaCloudClient,
    node: &str,
    target: &str,
    method: &str,
    svcs: &[String],
) -> EResult<Value> {
    #[derive(Serialize)]
    struct Payload<'a> {
        svcs: &'a [String],
    }
    client
        .call::<()>(node, target, method, Some(to_value(Payload { svcs })?))
        .await?;
    Ok(Value::Unit)
}

async fn process_items(
    client: &EvaCloudClient,
    node: &str,
    target: &str,
    method: &str,
    oids: &[String],
) -> EResult<Value> {
    #[derive(Serialize)]
    struct Payload<'a> {
        items: &'a [String],
    }
    client
        .call::<()>(
            node,
            target,
            method,
            Some(to_value(Payload { items: oids })?),
        )
        .await?;
    Ok(Value::Unit)
}

#[derive(Serialize)]
struct SvcDeployPayload<'a> {
    svcs: Vec<SvcPayload<'a>>,
}

#[derive(Serialize)]
struct SvcPayload<'a> {
    id: &'a str,
    params: &'a ServiceParams,
}

#[derive(Serialize)]
struct SvcDeployPayloadVals<'a> {
    svcs: &'a [Value],
}

#[derive(Serialize)]
struct ItemDeployPayload<'a> {
    items: Vec<&'a ItemConfig>,
}

impl<'a> From<&'a ItemConfig> for ItemDeployPayload<'a> {
    fn from(config: &'a ItemConfig) -> Self {
        Self {
            items: vec![config],
        }
    }
}

#[derive(Serialize)]
struct ItemDeployPayloadVals<'a> {
    items: &'a [Value],
}

impl<'a> TryFrom<&'a ServiceParams> for SvcDeployPayload<'a> {
    type Error = Error;
    fn try_from(params: &'a ServiceParams) -> EResult<Self> {
        if let Some(id) = params.id.as_ref() {
            if id.ends_with('.') {
                Err(Error::invalid_data("invalid ID"))
            } else {
                Ok(Self {
                    svcs: vec![SvcPayload { id, params }],
                })
            }
        } else {
            Err(Error::invalid_data("ID is not set"))
        }
    }
}

async fn s_call<S: Serialize>(
    client: &EvaCloudClient,
    node: &str,
    svc: &str,
    method: &str,
    params: S,
) -> EResult<Value> {
    client
        .call(node, svc, method, Some(to_value(params)?))
        .await
}

#[allow(clippy::too_many_lines)]
async fn do_process_command(client: Arc<EvaCloudClient>, nit: Nit) -> EResult<Value> {
    match nit.kind() {
        NitKind::StartItemWatcher(u, oid, int) => {
            let u = *u;
            let int = *int;
            let node: String = nit.node().to_owned();
            let oid: OID = oid.clone();
            let client = client.clone();
            let fut = tokio::spawn(async move {
                item_watcher(client, u, &node, &oid, int).await;
            });
            WATCHERS.lock().unwrap().insert(u, fut);
            Ok(Value::Unit)
        }
        NitKind::StopWatcher(u) => {
            if let Some(fut) = WATCHERS.lock().unwrap().remove(u) {
                fut.abort();
            }
            Ok(Value::Unit)
        }
        NitKind::StartActionWatcher(u, action_uuid, int) => {
            let u = *u;
            let int = *int;
            let node: String = nit.node().to_owned();
            let client = client.clone();
            let a_uuid = *action_uuid;
            let fut = tokio::spawn(async move {
                action_watcher(client, u, &node, a_uuid, int).await;
            });
            WATCHERS.lock().unwrap().insert(u, fut);
            Ok(Value::Unit)
        }
        NitKind::State => {
            let state = client
                .call::<Value>(nit.node(), SVC_CORE, "test", None)
                .await?;
            let node_list = client
                .call::<Value>(nit.node(), SVC_CORE, "node.list", None)
                .await?;
            Ok(Value::Seq(vec![state, node_list]))
        }
        NitKind::Log(filter) => {
            if let Some(f) = filter {
                client
                    .call::<Value>(nit.node(), SVC_CORE, "log.get", Some(to_value(f)?))
                    .await
            } else {
                Ok(Value::Seq(Vec::new()))
            }
        }
        NitKind::Actions(filter) => {
            if let Some(f) = filter {
                client
                    .call::<Value>(nit.node(), SVC_CORE, "action.list", Some(to_value(f)?))
                    .await
            } else {
                Ok(Value::Seq(Vec::new()))
            }
        }
        NitKind::Items(oid, node) => {
            #[derive(Serialize, Debug)]
            struct PayloadItemList<'a> {
                i: &'a str,
                src: Option<&'a str>,
            }
            if let Some(oid) = oid {
                let node = if let Some(node) = node {
                    if node.is_empty() || node == "*" || node == "#" {
                        None
                    } else {
                        Some(node.as_str())
                    }
                } else {
                    None
                };
                let payload = PayloadItemList { i: oid, src: node };
                client
                    .call::<Value>(nit.node(), SVC_CORE, "item.list", Some(to_value(payload)?))
                    .await
            } else {
                Ok(Value::Seq(Vec::new()))
            }
        }
        NitKind::ItemGetState(oid) => {
            s_call(
                &client,
                nit.node(),
                SVC_CORE,
                "item.state",
                ParamsId { i: oid.as_str() },
            )
            .await
        }
        NitKind::ItemGetConfig(oid) => {
            s_call(
                &client,
                nit.node(),
                SVC_CORE,
                "item.get_config",
                ParamsId { i: oid },
            )
            .await
        }
        NitKind::Broker => {
            client
                .call(nit.node(), ".broker", "client.list", None)
                .await
        }
        NitKind::Services => client.call(nit.node(), SVC_CORE, "svc.list", None).await,
        NitKind::Save => client.call(nit.node(), SVC_CORE, "save", None).await,
        NitKind::Restart => {
            client
                .call(nit.node(), SVC_CORE, "core.shutdown", None)
                .await
        }
        NitKind::SvcRestart(svcs) => {
            process_bulk(&client, nit.node(), SVC_CORE, "svc.restart", svcs).await
        }
        NitKind::SvcDestroy(svcs) => {
            process_svcs(&client, nit.node(), SVC_CORE, "svc.undeploy", svcs).await
        }
        NitKind::SvcPurge(svcs) => {
            process_svcs(&client, nit.node(), SVC_CORE, "svc.purge", svcs).await
        }
        NitKind::SvcGetParams(svc) => {
            s_call(
                &client,
                nit.node(),
                SVC_CORE,
                "svc.get_params",
                ParamsId { i: svc },
            )
            .await
        }
        NitKind::SvcGetParamsX(svc) => {
            let spoint_list = client
                .call::<Value>(nit.node(), SVC_CORE, "spoint.list", None)
                .await?;
            let params = s_call(
                &client,
                nit.node(),
                SVC_CORE,
                "svc.get_params",
                ParamsId { i: svc },
            )
            .await?;
            Ok(Value::Seq(vec![params, spoint_list]))
        }
        NitKind::SvcDeploySingle(params) => {
            client
                .call(
                    nit.node(),
                    SVC_CORE,
                    "svc.deploy",
                    Some(to_value(TryInto::<SvcDeployPayload>::try_into(&**params)?)?),
                )
                .await
        }
        NitKind::SvcDeployMultiple(svcs) => {
            client
                .call(
                    nit.node(),
                    SVC_CORE,
                    "svc.deploy",
                    Some(to_value(SvcDeployPayloadVals { svcs })?),
                )
                .await
        }
        NitKind::SvcGetInfo(svc) => client.call(nit.node(), svc, "info", None).await,
        NitKind::SvcCall(u, svc, method, payload) => {
            let node = nit.node().to_owned();
            let u = *u;
            let svc = svc.clone();
            let method = method.clone();
            let payload = payload.clone();
            tokio::spawn(async move {
                let result = client.call(&node, &svc, &method, payload).await;
                ui::command(ui::Command::ProcessSvcCallResult(u, result));
            });
            Ok(Value::Unit)
        }
        NitKind::ItemGetConfigX(oid) => {
            let items = client
                .call::<Value>(nit.node(), SVC_CORE, "svc.list", None)
                .await?;
            let params = s_call(
                &client,
                nit.node(),
                SVC_CORE,
                "item.get_config",
                ParamsId { i: oid },
            )
            .await?;
            Ok(Value::Seq(vec![params, items]))
        }
        NitKind::ItemDeploySingle(config) => {
            client
                .call(
                    nit.node(),
                    SVC_CORE,
                    "item.deploy",
                    Some(to_value(TryInto::<ItemDeployPayload>::try_into(
                        &**config,
                    )?)?),
                )
                .await
        }
        NitKind::ItemDeployMultiple(oids) => {
            client
                .call(
                    nit.node(),
                    SVC_CORE,
                    "item.deploy",
                    Some(to_value(ItemDeployPayloadVals { items: oids })?),
                )
                .await
        }
        NitKind::ItemDestroy(oids) => {
            process_items(&client, nit.node(), SVC_CORE, "item.undeploy", oids).await
        }
        NitKind::ItemAnnounce(oids) => {
            process_bulk(&client, nit.node(), SVC_CORE, "item.announce", oids).await
        }
        NitKind::ItemDisable(oids) => {
            process_bulk(&client, nit.node(), SVC_CORE, "item.disable", oids).await
        }
        NitKind::ItemEnable(oids) => {
            process_bulk(&client, nit.node(), SVC_CORE, "item.enable", oids).await
        }
        NitKind::UnitAction(p_action) => {
            client
                .call(nit.node(), SVC_CORE, "action", Some(to_value(p_action)?))
                .await
        }
        NitKind::LmacroRun(p_action) => {
            client
                .call(nit.node(), SVC_CORE, "run", Some(to_value(p_action)?))
                .await
        }
        NitKind::UnitActionToggle(oid) => {
            client
                .call(
                    nit.node(),
                    SVC_CORE,
                    "action.toggle",
                    Some(to_value(ParamsId { i: oid })?),
                )
                .await
        }
        NitKind::LvarSet(oids, p_set) => process_lvar_set(&client, nit.node(), oids, p_set).await,
        NitKind::LvarReset(oids) => {
            process_bulk(&client, nit.node(), SVC_CORE, "lvar.reset", oids).await
        }
        NitKind::LvarClear(oids) => {
            process_bulk(&client, nit.node(), SVC_CORE, "lvar.clear", oids).await
        }
        NitKind::LvarToggle(oids) => {
            process_bulk(&client, nit.node(), SVC_CORE, "lvar.toggle", oids).await
        }
        NitKind::LvarIncr(oids) => {
            process_bulk(&client, nit.node(), SVC_CORE, "lvar.incr", oids).await
        }
        NitKind::LvarDecr(oids) => {
            process_bulk(&client, nit.node(), SVC_CORE, "lvar.decr", oids).await
        }
        NitKind::SPoints => client.call(nit.node(), SVC_CORE, "spoint.list", None).await,
    }
}

async fn process_commands(
    client: Arc<EvaCloudClient>,
    mut rx: mpsc::Receiver<(Nit, CommandReplyTx)>,
) {
    while let Some((nit, reply)) = rx.recv().await {
        let result = do_process_command(client.clone(), nit).await;
        let _r = reply.send(result);
    }
}

pub fn connect(opts: ConnectionOptions) {
    disconnect();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let (tx, rx) = oneshot::channel::<()>();
            let fut = tokio::spawn(async move {
                launch_connection(&opts.path, tx, opts.timeout, opts.credentials).await;
                crate::CLIENT_CHANNEL.lock().unwrap().take();
            });
            crate::CONNECTION.lock().unwrap().replace(fut);
            let _r = rx.await;
        });
    });
}

pub fn disconnect() {
    crate::CLIENT_CHANNEL.lock().unwrap().take();
    if let Some(fut) = crate::CONNECTION.lock().unwrap().take() {
        fut.abort();
    }
    if let Some(fut) = crate::NIT_HANDLER.lock().unwrap().take() {
        fut.abort();
    }
    crate::LAST_NIT.lock().unwrap().take();
    ui::command(ui::Command::MarkDisconnected);
    let mut watchers = WATCHERS.lock().unwrap();
    for fut in watchers.values() {
        fut.abort();
    }
    watchers.clear();
}

pub fn call<T: DeserializeOwned>(nit: Nit) -> EResult<T> {
    let ch = get_client_channel()?;
    let (tx, rx) = mpsc_std::sync_channel(1);
    ch.try_send((nit, tx))
        .map_err(|_| Error::failed("Operations pending"))?;
    let val = T::deserialize(rx.recv().map_err(Error::failed)??)?;
    Ok(val)
}

pub fn item_state(node: &str, oid: OID) -> EResult<ItemState> {
    let mut res: Vec<ItemState> = call(Arc::new(NitData::new_item_get_state(node, oid.clone())))?;
    if res.len() == 1 {
        let state = res.remove(0);
        if state.oid == oid {
            Ok(state)
        } else {
            Err(Error::invalid_data("OID mismatch"))
        }
    } else {
        Err(Error::invalid_data("invalid payload received"))
    }
}

fn get_client_channel() -> EResult<CommandTx> {
    if let Some(client) = crate::CLIENT_CHANNEL.lock().unwrap().as_ref() {
        Ok(client.clone())
    } else {
        Err(Error::io("Not connected"))
    }
}

pub type CommandReplyTx = mpsc_std::SyncSender<EResult<Value>>;
pub type CommandTx = mpsc::Sender<(Nit, CommandReplyTx)>;
