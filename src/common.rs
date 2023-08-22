use crate::ui;
use clap::Parser;
use cpp_core::CppBox;
use eva_client::VersionInfo;
use eva_common::prelude::*;
use qt_core::{QListOfInt, QPtr};
use qt_widgets::{QSplitter, QTableWidget};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::os::raw::c_int;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

pub const LAUNCHER_MAIN: &str = "eva.launcher.main";
pub const LAUNCHER_PFX: &str = "eva.launcher.";
pub const DEFAULT_TIMEOUT_SEC: f64 = 5.0;

pub const DEFAULT_BUS_TYPE: &str = "native";
pub const DEFAULT_BUS_PATH: &str = "var/bus.ipc";
pub const DEFAULT_BUS_PING_INTERVAL_SEC: f64 = 1.0;

#[cfg(target_os = "windows")]
pub const CRLF: &str = "\r\n";
#[cfg(not(target_os = "windows"))]
pub const CRLF: &str = "\n";

#[derive(Parser)]
pub struct Args {
    #[clap(
        short = 'C',
        long = "connection-path",
        help = "BUS/RT socket or HTTP API URL"
    )]
    connection_path: Option<String>,
    #[clap(short = 'T', long = "timeout")]
    connection_timeout: Option<f64>,
    #[clap(short = 'U', long = "user")]
    user: Option<String>,
    #[clap(short = 'P', long = "password")]
    password: Option<String>,
}

impl Args {
    pub fn connection_options(&self) -> Option<ConnectionOptions> {
        self.connection_path.as_ref().map(|path| ConnectionOptions {
            path: if let Some(p) = path.strip_prefix("rt://") {
                p.to_owned()
            } else {
                path.clone()
            },
            credentials: self.user.as_ref().map(|user| {
                (
                    user.clone(),
                    self.password
                        .as_ref()
                        .map_or_else(<_>::default, Clone::clone),
                )
            }),
            timeout: Duration::from_secs_f64(
                self.connection_timeout.unwrap_or(DEFAULT_TIMEOUT_SEC),
            ),
        })
    }
}

pub unsafe fn new_size(x: c_int, y: c_int) -> CppBox<QListOfInt> {
    let sizes = QListOfInt::new();
    sizes.append_int(&x);
    sizes.append_int(&y);
    sizes
}

pub unsafe fn splitter_sizes(splitter: &QPtr<QSplitter>) -> Option<(c_int, c_int)> {
    let curr_sizes = splitter.sizes();
    if curr_sizes.length() == 2 {
        let size_left = curr_sizes.take_first();
        let size_right = curr_sizes.take_first();
        Some((size_left, size_right))
    } else {
        None
    }
}

pub fn load_yaml<O: DeserializeOwned>(fname: &str) -> EResult<Option<O>> {
    let path = Path::new(fname);
    if path.exists() {
        let mut f = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        Ok(Some(
            serde_yaml::from_slice(&buf).map_err(Error::invalid_data)?,
        ))
    } else {
        Ok(None)
    }
}

pub fn save_yaml<S: Serialize>(fname: &str, value: &S) -> EResult<()> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(false)
        .truncate(true)
        .write(true)
        .open(fname)?;
    f.write_all(&serde_yaml::to_vec(value).map_err(Error::invalid_data)?)?;
    Ok(())
}

#[derive(Deserialize)]
pub struct SPointInfo {
    pub name: String,
    pub port: String,
    pub source: String,
    #[serde(flatten)]
    pub info: VersionInfo,
}

impl SPointInfo {
    pub fn short_name(&self) -> Option<&str> {
        self.name.strip_prefix("eva.spoint.")
    }
}

fn default_workers() -> c_int {
    1
}

fn default_launcher() -> String {
    LAUNCHER_MAIN.to_owned()
}

#[derive(Serialize, Clone)]
pub struct PayloadLvarSet {
    pub i: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ItemStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Serialize)]
pub struct PayloadAction {
    pub i: Option<String>,
    pub params: eva_common::actions::Params,
}

#[derive(Serialize, Clone, Debug, Eq, PartialEq)]
pub struct ActionFilter {
    pub i: Option<String>,
    pub sq: Option<String>,
    pub svc: Option<String>,
    pub time: Option<u32>,
    pub limit: Option<u32>,
}
#[derive(Serialize, Clone, Debug, Eq, PartialEq)]
pub struct UsersFilter {
    pub svc: Option<String>,
}

#[derive(Deserialize)]
pub struct ActionRecordFull {
    pub uuid: uuid::Uuid,
    pub oid: String,
    pub status: String,
    pub node: String,
    pub svc: String,
    pub finished: bool,
    #[serde(default)]
    pub time: BTreeMap<String, f64>,
    pub exitcode: Option<i16>,
    pub params: Option<Value>,
    pub out: Option<Value>,
    pub err: Option<Value>,
}

impl ActionRecordFull {
    pub fn elapsed(&self) -> Option<f64> {
        action_elapsed(self.finished, &self.time)
    }
}

#[derive(Deserialize)]
pub struct ActionRecord {
    #[serde(default)]
    pub time: BTreeMap<String, f64>,
    pub uuid: uuid::Uuid,
    pub oid: String,
    pub status: String,
    pub node: String,
    pub svc: String,
    pub finished: bool,
}

fn action_elapsed(finished: bool, time: &BTreeMap<String, f64>) -> Option<f64> {
    if finished {
        let min = time.values().fold(f64::INFINITY, |a, &b| a.min(b));
        let max = time.values().fold(0f64, |a, &b| a.max(b));
        Some(max - min)
    } else {
        None
    }
}

impl ActionRecord {
    pub fn time(&self) -> Option<f64> {
        self.time.get("created").copied()
    }
    pub fn elapsed(&self) -> Option<f64> {
        action_elapsed(self.finished, &self.time)
    }
}

#[derive(Serialize, Clone, Debug, Eq, PartialEq)]
pub struct LogFilter {
    pub level: Option<String>,
    pub time: Option<u32>,
    pub limit: Option<u32>,
    pub module: Option<String>,
    pub rx: Option<String>,
}

#[derive(Deserialize)]
pub struct LogRecord {
    pub dt: String,
    pub l: u8,
    pub lvl: String,
    #[serde(rename = "mod")]
    pub module: Option<String>,
    pub msg: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ServiceParams {
    #[serde(skip)]
    pub id: Option<String>,
    #[serde(default)]
    pub command: String,
    pub prepare_command: Option<String>,
    #[serde(default = "eva_common::tools::default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub react_to_fail: bool,
    #[serde(default)]
    pub call_tracing: bool,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub config: Value,
    #[serde(default = "default_workers")]
    pub workers: c_int,
    #[serde(default = "default_launcher")]
    pub launcher: String,
    #[serde(default)]
    pub timeout: TimeoutConfig,
    #[serde(default)]
    pub bus: BusConfig,
}

impl ServiceParams {
    pub fn load_from_disk(fname: &str) -> EResult<Self> {
        let mut f = std::fs::File::open(fname)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        serde_yaml::from_slice(&buf).map_err(Error::failed)
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct TimeoutConfig {
    pub default: Option<f64>,
    pub startup: Option<f64>,
    pub shutdown: Option<f64>,
}

fn default_bus_path() -> String {
    DEFAULT_BUS_PATH.to_owned()
}

fn default_bus_type() -> String {
    DEFAULT_BUS_TYPE.to_owned()
}

#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_possible_wrap)]
fn default_buf_size() -> i32 {
    busrt::DEFAULT_BUF_SIZE as i32
}

#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_possible_wrap)]
fn default_buf_ttl() -> i32 {
    busrt::DEFAULT_BUF_TTL.as_micros() as i32
}

#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_possible_wrap)]
fn default_bus_queue_size() -> i32 {
    busrt::DEFAULT_QUEUE_SIZE as i32
}

fn default_bus_ping_interval() -> f64 {
    DEFAULT_BUS_PING_INTERVAL_SEC
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BusConfig {
    #[serde(default = "default_buf_size")]
    pub buf_size: i32,
    #[serde(default = "default_buf_ttl")]
    pub buf_ttl: i32,
    #[serde(default = "default_bus_path")]
    pub path: String,
    #[serde(default = "default_bus_ping_interval")]
    pub ping_interval: f64,
    #[serde(default = "default_bus_queue_size")]
    pub queue_size: i32,
    pub timeout: Option<f64>,
    #[serde(rename = "type", default = "default_bus_type")]
    pub kind: String,
}

impl Default for BusConfig {
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_possible_wrap)]
    fn default() -> Self {
        Self {
            buf_size: busrt::DEFAULT_BUF_SIZE as i32,
            buf_ttl: busrt::DEFAULT_BUF_TTL.as_micros() as i32,
            path: default_bus_path(),
            ping_interval: DEFAULT_BUS_PING_INTERVAL_SEC,
            queue_size: busrt::DEFAULT_QUEUE_SIZE as i32,
            kind: default_bus_type(),
            timeout: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionOptions {
    pub path: String,
    pub credentials: Option<(String, String)>,
    pub timeout: Duration,
}

impl From<ConnectionOptions> for ConnectionOptionsSaved {
    fn from(c: ConnectionOptions) -> Self {
        Self {
            path: c.path,
            login: if let Some((login, _)) = c.credentials {
                login
            } else {
                String::new()
            },
            timeout: c.timeout.as_secs_f64(),
        }
    }
}

impl From<ConnectionOptionsSaved> for ConnectionOptions {
    fn from(c: ConnectionOptionsSaved) -> Self {
        Self {
            path: c.path,
            credentials: Some((c.login, String::new())),
            timeout: Duration::from_secs_f64(c.timeout),
        }
    }
}

#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Deserialize, Serialize)]
pub struct Config {
    connection: ConnectionOptionsSaved,
    #[serde(default)]
    auto_reload: f64,
    ui: UiConfig,
}

impl Config {
    pub fn new(con_opts: ConnectionOptions, auto_reload: f64) -> Self {
        Self {
            connection: con_opts.into(),
            auto_reload,
            ui: <_>::default(),
        }
    }
    pub fn set_auto_reload(&mut self, r: f64) {
        self.auto_reload = r;
    }
    pub fn set_connection(&mut self, opts: &ConnectionOptions) {
        self.connection = opts.clone().into();
    }
    pub fn set_main_window_size(&mut self, w: c_int, h: c_int) {
        self.ui.main.size.s1 = w;
        self.ui.main.size.s2 = h;
    }
    pub fn set_s_workspace(&mut self, s1: c_int, s2: c_int) {
        self.ui.s_workspace.size.s1 = s1;
        self.ui.s_workspace.size.s2 = s2;
    }
    pub fn set_s_tables(&mut self, s1: c_int, s2: c_int) {
        self.ui.s_tables.size.s1 = s1;
        self.ui.s_tables.size.s2 = s2;
    }
    pub unsafe fn qt_apply(&self, u: &Rc<ui::Ui>) {
        let size = &self.ui.main.size;
        if size.s1 > 0 && size.s2 > 0 {
            u.window.widget.resize_2a(size.s1, size.s2);
        }
        let size = &self.ui.s_workspace.size;
        if size.s1 > 0 && size.s2 > 0 {
            u.window
                .splitter_workspace
                .set_sizes(&new_size(size.s1, size.s2));
        }
        let size = &self.ui.s_tables.size;
        if size.s1 > 0 && size.s2 > 0 {
            u.window
                .splitter_tables
                .set_sizes(&new_size(size.s1, size.s2));
        }
        u.window.auto_reload.set_value(self.auto_reload);
        u.dialog_connect.set_data(self.connection.clone().into());
    }
    pub fn save_to_disk(&self) {
        match serde_yaml::to_vec(&self) {
            Ok(data) => {
                if let Some(file) = crate::CONFIG_FILE.as_ref() {
                    if let Some(dir) = file.parent() {
                        if let Err(e) = std::fs::create_dir_all(dir) {
                            eprintln!(
                                "unable to create directory {}: {}",
                                dir.to_string_lossy(),
                                e
                            );
                        } else {
                            match std::fs::OpenOptions::new()
                                .create(true)
                                .append(false)
                                .truncate(true)
                                .write(true)
                                .open(file)
                            {
                                Ok(mut f) => {
                                    f.write_all(&data).unwrap();
                                    f.flush().unwrap();
                                }
                                Err(e) => {
                                    eprintln!(
                                        "unable to open file {}: {}",
                                        dir.to_string_lossy(),
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Unable to serialize config: {}", e);
            }
        }
    }
    pub fn load_from_disk() -> EResult<Option<Config>> {
        if let Some(file) = crate::CONFIG_FILE.as_ref() {
            if file.exists() {
                let mut buf = Vec::new();
                std::fs::File::open(file)?.read_to_end(&mut buf)?;
                Ok(Some(
                    serde_yaml::from_slice(&buf).map_err(Error::invalid_data)?,
                ))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}

#[derive(Deserialize, Serialize, Default)]
pub struct UiConfig {
    main: MainWindowConfig,
    s_workspace: SplitterConfig,
    s_tables: SplitterConfig,
}

#[derive(Deserialize, Serialize, Default)]
pub struct MainWindowConfig {
    size: SizeConfig,
}

#[derive(Deserialize, Serialize, Default)]
pub struct SplitterConfig {
    size: SizeConfig,
}

#[derive(Deserialize, Serialize, Default)]
pub struct SizeConfig {
    s1: c_int,
    s2: c_int,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ConnectionOptionsSaved {
    path: String,
    login: String,
    timeout: f64,
}

#[derive(Deserialize)]
pub struct BrokerInfo {
    pub clients: Vec<BrokerClientInfo>,
}

#[derive(Deserialize)]
pub struct BrokerClientInfo {
    pub name: String,
    pub kind: String,
    pub source: Option<String>,
    pub port: Option<String>,
    pub r_frames: u64,
    pub w_frames: u64,
    pub r_bytes: u64,
    pub w_bytes: u64,
    pub queue: u64,
    pub instances: u64,
}

#[derive(Deserialize)]
pub struct UserInfo {
    pub login: String,
    pub acls: Vec<String>,
}

#[allow(dead_code)]
#[derive(Deserialize, Clone)]
pub struct NodeInfo {
    pub name: String,
    pub online: bool,
    pub remote: bool,
    pub svc: Option<String>,
    pub info: Option<VersionInfo>,
}

#[derive(Deserialize, Clone)]
pub struct SvcData {
    pub id: String,
    pub launcher: String,
    pub status: String,
    pub pid: Option<u32>,
}

#[derive(Deserialize)]
pub struct SvcInfo {
    pub author: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub methods: BTreeMap<String, SvcMethodInfo>,
}

#[derive(Deserialize)]
pub struct SvcMethodInfo {
    pub description: Option<String>,
    #[serde(default)]
    pub params: BTreeMap<String, SvcMethodInfoParam>,
}

#[derive(Deserialize)]
pub struct SvcMethodInfoParam {
    #[serde(default)]
    pub required: bool,
}

#[derive(Deserialize)]
pub struct ItemState {
    pub oid: OID,
    pub status: ItemStatus,
    pub value: Option<Value>,
}

#[derive(Deserialize, Clone)]
pub struct ItemInfo {
    pub oid: OID,
    pub status: Option<ItemStatus>,
    pub value: Option<Value>,
    pub t: Option<f64>,
    pub node: String,
    pub connected: bool,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ItemConfig {
    pub oid: OID,
    #[serde(default)]
    pub enabled: bool,
    pub meta: Option<Value>,
    pub action: Option<ItemActionConfig>,
    pub logic: Option<ItemLogicConfig>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ItemActionConfig {
    pub timeout: Option<f64>,
    pub svc: Option<String>,
    pub config: Option<Value>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ItemLogicConfig {
    pub range: Option<eva_common::logic::Range>,
}

impl Ord for NodeInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.remote {
            self.name.cmp(&other.name)
        } else {
            std::cmp::Ordering::Less
        }
    }
}

impl PartialOrd for NodeInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for NodeInfo {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for NodeInfo {}

pub type Nit = Arc<NitData>;

pub struct NitData {
    node: String,
    kind: NitKind,
}

impl fmt::Display for NitData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}->{}", self.node, self.kind)
    }
}

impl NitData {
    pub fn new_state(node: &str) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::State,
        }
    }
    pub fn new_log(node: &str, filter: LogFilter) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::Log(Some(filter)),
        }
    }
    pub fn new_users(node: &str, filter: UsersFilter) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::Users(Some(filter)),
        }
    }
    pub fn new_actions(node: &str, filter: ActionFilter) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::Actions(Some(filter)),
        }
    }
    pub fn new_save(node: &str) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::Save,
        }
    }
    pub fn new_restart(node: &str) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::Restart,
        }
    }
    pub fn new_services(node: &str) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::Services,
        }
    }
    pub fn new_spoints(node: &str) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::SPoints,
        }
    }
    pub fn new_svc_restart(node: &str, svcs: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::SvcRestart(svcs),
        }
    }
    pub fn new_svc_destroy(node: &str, svcs: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::SvcDestroy(svcs),
        }
    }
    pub fn new_svc_purge(node: &str, svcs: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::SvcPurge(svcs),
        }
    }
    pub fn new_svc_get_params(node: &str, svc: String) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::SvcGetParams(svc),
        }
    }
    pub fn new_svc_get_params_x(node: &str, svc: String) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::SvcGetParamsX(svc),
        }
    }
    pub fn new_svc_get_info(node: &str, svc: String) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::SvcGetInfo(svc),
        }
    }
    pub fn new_svc_call(
        u: uuid::Uuid,
        node: &str,
        svc: String,
        method: String,
        payload: Option<Value>,
    ) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::SvcCall(u, svc, method, payload),
        }
    }
    pub fn new_item_get_state(node: &str, oid: OID) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::ItemGetState(oid),
        }
    }
    pub fn new_item_get_config_x(node: &str, oid: String) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::ItemGetConfigX(oid),
        }
    }
    pub fn new_svc_deploy(node: &str, params: ServiceParams) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::SvcDeploySingle(Box::new(params)),
        }
    }
    pub fn new_svc_deploy_multi(node: &str, svcs: Vec<Value>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::SvcDeployMultiple(svcs),
        }
    }
    pub fn new_item_list(node: &str, oid: Option<String>, item_node: Option<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::Items(oid, item_node),
        }
    }
    pub fn new_item_get_config(node: &str, oid: String) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::ItemGetConfig(oid),
        }
    }
    pub fn new_item_deploy(node: &str, config: ItemConfig) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::ItemDeploySingle(Box::new(config)),
        }
    }
    pub fn new_item_deploy_multi(node: &str, items: Vec<Value>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::ItemDeployMultiple(items),
        }
    }
    pub fn new_item_announce(node: &str, oids: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::ItemAnnounce(oids),
        }
    }
    pub fn new_item_destroy(node: &str, oids: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::ItemDestroy(oids),
        }
    }
    pub fn new_item_disable(node: &str, oids: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::ItemDisable(oids),
        }
    }
    pub fn new_item_enable(node: &str, oids: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::ItemEnable(oids),
        }
    }
    pub fn new_unit_action_toggle(node: &str, oid: String) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::UnitActionToggle(oid),
        }
    }
    pub fn new_unit_action(node: &str, p_action: PayloadAction) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::UnitAction(p_action),
        }
    }
    pub fn new_lmacro_run(node: &str, p_action: PayloadAction) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::LmacroRun(p_action),
        }
    }
    pub fn new_lvar_set(node: &str, oids: Vec<String>, p_set: PayloadLvarSet) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::LvarSet(oids, p_set),
        }
    }
    pub fn new_lvar_reset(node: &str, oids: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::LvarReset(oids),
        }
    }
    pub fn new_lvar_clear(node: &str, oids: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::LvarClear(oids),
        }
    }
    pub fn new_lvar_toggle(node: &str, oids: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::LvarToggle(oids),
        }
    }
    pub fn new_lvar_incr(node: &str, oids: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::LvarIncr(oids),
        }
    }
    pub fn new_lvar_decr(node: &str, oids: Vec<String>) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::LvarDecr(oids),
        }
    }
    pub fn start_item_watcher(u: uuid::Uuid, node: &str, oid: OID, int: Duration) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::StartItemWatcher(u, oid, int),
        }
    }
    pub fn start_action_watcher(
        u: uuid::Uuid,
        node: &str,
        action_uuid: uuid::Uuid,
        int: Duration,
    ) -> Self {
        Self {
            node: node.to_owned(),
            kind: NitKind::StartActionWatcher(u, action_uuid, int),
        }
    }
    pub fn stop_watcher(u: uuid::Uuid) -> Self {
        Self {
            node: String::new(),
            kind: NitKind::StopWatcher(u),
        }
    }
    pub fn node(&self) -> &str {
        &self.node
    }
    pub fn kind(&self) -> &NitKind {
        &self.kind
    }
    #[allow(dead_code)]
    pub fn set_kind(&mut self, kind: NitKind) {
        self.kind = kind;
    }
}

pub enum NitKind {
    State,
    Services,
    Items(Option<String>, Option<String>),
    Log(Option<LogFilter>),
    Users(Option<UsersFilter>),
    // Users,
    Actions(Option<ActionFilter>),
    ItemGetConfig(String),
    ItemGetState(OID),
    ItemDeploySingle(Box<ItemConfig>),
    ItemDeployMultiple(Vec<Value>),
    ItemAnnounce(Vec<String>),
    ItemDestroy(Vec<String>),
    ItemDisable(Vec<String>),
    ItemEnable(Vec<String>),
    LvarSet(Vec<String>, PayloadLvarSet),
    LvarReset(Vec<String>),
    LvarClear(Vec<String>),
    LvarToggle(Vec<String>),
    LvarIncr(Vec<String>),
    LvarDecr(Vec<String>),
    UnitActionToggle(String),
    UnitAction(PayloadAction),
    LmacroRun(PayloadAction),
    Broker,
    Save,
    Restart,
    SvcRestart(Vec<String>),
    SvcDestroy(Vec<String>),
    SvcPurge(Vec<String>),
    SvcGetParams(String),
    SvcGetParamsX(String),
    ItemGetConfigX(String),
    SvcDeploySingle(Box<ServiceParams>),
    SvcDeployMultiple(Vec<Value>),
    SvcGetInfo(String),
    SvcCall(uuid::Uuid, String, String, Option<Value>),
    SPoints,
    StartItemWatcher(uuid::Uuid, OID, Duration),
    StartActionWatcher(uuid::Uuid, uuid::Uuid, Duration), // second UUID = action UUID
    StopWatcher(uuid::Uuid),
}

impl fmt::Display for NitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                NitKind::Save => "save",
                NitKind::Restart => "restart",
                NitKind::SvcRestart(_) => "svc.restart",
                NitKind::SvcDestroy(_) => "svc.undeploy",
                NitKind::SvcPurge(_) => "svc.purge",
                NitKind::ItemDestroy(_) => "item.undeploy",
                NitKind::ItemDisable(_) => "item.disable",
                NitKind::ItemEnable(_) => "item.enable",
                NitKind::ItemAnnounce(_) => "item.announce",
                NitKind::LvarSet(_, _) => "lvar.set",
                NitKind::LvarReset(_) => "lvar.reset",
                NitKind::LvarClear(_) => "lvar.clear",
                NitKind::LvarToggle(_) => "lvar.toggle",
                NitKind::LvarIncr(_) => "lvar.incr",
                NitKind::LvarDecr(_) => "lvar.decr",
                _ => "",
            }
        )
    }
}

pub fn nd_from_path(path: &[&str]) -> Option<NitData> {
    if let Some(node_name) = path.first() {
        match path.get(1) {
            #[allow(clippy::match_single_binding)]
            Some(node_leaf) => match *node_leaf {
                "broker" => Some(NitData {
                    node: (*node_name).to_owned(),
                    kind: NitKind::Broker,
                }),
                "services" => Some(NitData {
                    node: (*node_name).to_owned(),
                    kind: NitKind::Services,
                }),
                "spoints" => Some(NitData {
                    node: (*node_name).to_owned(),
                    kind: NitKind::SPoints,
                }),
                "items" => Some(NitData {
                    node: (*node_name).to_owned(),
                    kind: NitKind::Items(None, None),
                }),
                "log" => Some(NitData {
                    node: (*node_name).to_owned(),
                    kind: NitKind::Log(None),
                }),
                "actions" => Some(NitData {
                    node: (*node_name).to_owned(),
                    kind: NitKind::Actions(None),
                }),
                "users" => Some(NitData {
                    node: (*node_name).to_owned(),
                    kind: NitKind::Users(None),
                }),
                _ => {
                    dbg!(node_name, node_leaf);
                    None
                }
            },
            None => Some(NitData {
                node: (*node_name).to_owned(),
                kind: NitKind::State,
            }),
        }
    } else {
        None
    }
}

pub fn spent_time(time: u64) -> String {
    if time < 60 {
        format!("{} sec", time)
    } else if time < 3600 {
        let mins = time / 60;
        format!("{}m {}s", mins, time - mins * 60)
    } else if time < 86400 {
        let hours = time / 3600;
        let mins = (time - hours * 3600) / 60;
        format!("{}h {}m {}s", hours, mins, time - hours * 3600 - mins * 60)
    } else {
        let days = time / 86400;
        let hours = (time - days * 86400) / 3600;
        let mins = (time - days * 86400 - hours * 3600) / 60;
        format!(
            "{}d {}h {}m {}s",
            days,
            hours,
            mins,
            time - days * 86400 - hours * 3600 - mins * 60
        )
    }
}

pub unsafe fn copy_from_table(tables: &[&QPtr<QTableWidget>]) -> String {
    let mut result = String::default();
    for table in tables {
        if !table.current_item().is_null() {
            let items = table.selected_items();
            let mut prev_row: Option<c_int> = None;
            loop {
                if items.is_empty() {
                    break;
                }
                let item = items.take_first();
                if item.is_null() {
                    break;
                }
                let row = item.row();
                if let Some(prev) = prev_row {
                    if row == prev {
                        result += "\t";
                    } else {
                        result += CRLF;
                        prev_row.replace(row);
                    }
                } else {
                    prev_row.replace(row);
                }
                write!(result, "{}", item.text().to_std_string()).unwrap();
            }
            break;
        }
    }

    result
}
