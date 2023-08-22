use crate::common::{
    copy_from_table, load_yaml, new_size, save_yaml, splitter_sizes, ActionFilter, ActionRecord,
    Args, Config, ItemConfig, LogFilter, Nit, NitData, NitKind, NodeInfo, SPointInfo,
    ServiceParams, SvcData, SvcInfo, UsersFilter,
};
use crate::output;
use crate::smart_table;
use crate::{
    bus,
    forms::{self, ExportKind, QInputX},
};
use arboard::Clipboard;
use cpp_core::{CppBox, Ptr, Ref, StaticUpcast};
use eva_common::prelude::*;
use qt_core::{
    qs, slot, QBox, QObject, QPoint, QPtr, QSortFilterProxyModel, QString, QTimer, SlotNoArgs,
    SlotOfDouble, SlotOfQString,
};
use qt_gui::{QIcon, QPixmap, QStandardItemModel};
use qt_widgets::{
    QAction, QApplication, QFileDialog, QMenu, QMessageBox, QTableWidget, QTableWidgetItem,
    QTableWidgetSelectionRange, QTreeWidget, QTreeWidgetItem,
};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fmt::Write as _;
use std::os::raw::c_int;
use std::rc::Rc;
use std::sync::atomic;
use std::sync::mpsc as mpsc_std;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const ERR_LOAD_SPOINTS: &str = "Unable to load list of secondary points";

const KIND_SVC: &str = "service(s)";
const KIND_ITEM: &str = "item(s)";
const KIND_UNIT: &str = "unit(s)";
const KIND_LVAR: &str = "lvar(s)";
const KIND_LMACRO: &str = "lmacro(s)";

const MAX_CONFIRM: usize = 10;

macro_rules! format_err {
    ($err: expr) => {
        format!(
            r#"<span style="color: red; font-weight: bold">{}</span>"#,
            $err
        )
    };
}

pub fn set_status(status: impl std::fmt::Display, kind: StatusKind) {
    command(Command::SetStatus(status.to_string(), kind));
}

pub fn command(c: Command) {
    unsafe {
        if crate::UI_TX.get().unwrap().lock().unwrap().send(c).is_err() {
            std::process::exit(5);
        }
    }
}

pub enum Command {
    SetStatus(String, StatusKind),
    MarkConnected(String, Vec<NodeInfo>),
    MarkDisconnected,
    ProcessNit(Nit),
    ProcessItemWatch(uuid::Uuid, Value),
    ProcessActionWatch(uuid::Uuid, Value),
    ProcessSvcCallResult(uuid::Uuid, EResult<Value>),
}

struct NodeTreeItem {
    primary: CppBox<QTreeWidgetItem>,
    secondaries: Vec<CppBox<QTreeWidgetItem>>,
}

impl Drop for NodeTreeItem {
    fn drop(&mut self) {
        self.secondaries.clear();
    }
}

impl NodeTreeItem {
    unsafe fn new(
        tree: &QPtr<QTreeWidget>,
        name: &str,
        auto_expand: bool,
        icon: &CppBox<QIcon>,
    ) -> Self {
        let primary = QTreeWidgetItem::from_q_tree_widget(tree);
        primary.set_text(0, &qs(name));
        primary.set_icon(0, icon);
        if auto_expand {
            tree.set_item_selected(&primary, true);
            tree.set_item_expanded(&primary, true);
        }
        Self {
            primary,
            secondaries: Vec::new(),
        }
    }
    unsafe fn add(&mut self, name: &str, icon: &CppBox<QIcon>) {
        let nl_leaf = QTreeWidgetItem::from_q_tree_widget_item(&self.primary);
        nl_leaf.set_text(0, &qs(name));
        nl_leaf.set_icon(0, icon);
        self.secondaries.push(nl_leaf);
    }
}

fn find_export_entry<'a>(
    data: &'a mut BTreeMap<Value, Value>,
    key: &str,
    cloud_deploy: bool,
    cloud_node: Option<&str>,
) -> EResult<&'a mut Vec<Value>> {
    macro_rules! process_entry {
        ($map: expr) => {{
            let entry = $map
                .entry(Value::String(key.to_owned()))
                .or_insert_with(|| Value::Seq(Vec::new()));
            if let Value::Seq(ref mut v) = entry {
                return Ok(v);
            }
            return Err(Error::invalid_data(format!("{} entry is not a seq", key)));
        }};
    }
    if cloud_deploy {
        let content: &mut Value = data
            .entry(Value::String("content".to_owned()))
            .or_insert_with(|| Value::Seq(Vec::new()));
        if let Value::Seq(ref mut c) = content {
            let node_key = Value::String("node".to_owned());
            if let Some(pos) = c.iter().position(|el| {
                if let Value::Map(ref map) = el {
                    if let Some(node) = map.get(&node_key) {
                        &node.to_string() == cloud_node.as_ref().unwrap()
                        //remutation is safe as the second block is not executed
                        //process_entry!(unsafe { as_mut(map) });
                    } else {
                        false
                    }
                    //}
                    //} else {
                    //return Err(Error::invalid_data("content entry is not a map"));
                } else {
                    false
                }
            }) {
                if let Value::Map(ref mut map) = c[pos] {
                    process_entry!(map);
                }
                panic!();
            }
            let mut deploy_map = BTreeMap::new();
            deploy_map.insert(
                node_key,
                Value::String((*cloud_node.as_ref().unwrap()).to_owned()),
            );
            // remutation is safe as the first block wasn't been executed cuz the entry hadn't been
            // found
            c.push(Value::Map(deploy_map));
            if let Value::Map(ref mut map) = c.last_mut().unwrap() {
                process_entry!(map)
            }
            panic!();
        } else {
            Err(Error::invalid_data("Section content is not a seq"))
        }
    } else {
        process_entry!(data);
    }
}

trait QResX {
    unsafe fn selected_resources(&self) -> Option<Vec<String>>;
}

impl QResX for QTableWidget {
    unsafe fn selected_resources(&self) -> Option<Vec<String>> {
        let items = self.selected_items();
        let mut rows: Vec<c_int> = Vec::new();
        if items.is_empty() {
            None
        } else {
            while !items.is_empty() {
                let item = items.take_first();
                let row = item.row();
                if !rows.contains(&row) {
                    rows.push(row);
                }
            }
            let mut resources: Vec<String> = Vec::new();
            for row in rows {
                let item = self.item(row, 0);
                if !item.is_null() {
                    resources.push(item.text().to_std_string());
                }
            }
            Some(resources)
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum StatusKind {
    //Dimmed,
    Info,
    //Okay,
    Error,
}

pub struct Ui {
    pub(crate) window: forms::Main,
    pub(crate) dialog_connect: forms::DialogConnect,
    dialog_lvar_set: Rc<forms::DialogLvarSet>,
    dialog_unit_action: Rc<forms::DialogUnitAction>,
    dialog_about: Rc<forms::DialogAbout>,
    dialog_export: Rc<forms::DialogExport>,
    busy: Rc<forms::Busy>,
    _source_model: QBox<QStandardItemModel>,
    _proxy_model: QBox<QSortFilterProxyModel>,
    cleanup_timer: QBox<QTimer>,
    cmd_rx: mpsc_std::Receiver<Command>,
    tree_items: Mutex<HashMap<String, NodeTreeItem>>,
    pub(crate) primary_table_items: Mutex<Vec<smart_table::Item>>,
    pub(crate) secondary_table_items: Mutex<Vec<smart_table::Item>>,
    auto_reload_timer: Mutex<Option<QBox<QTimer>>>,
    auto_reload_auto_suspended: atomic::AtomicBool,
    title: String,
    config: Mutex<Option<Config>>,
    action_timer: QBox<QTimer>,
    svc_edit_dialogs: forms::DialogFactory<forms::DialogSvcEdit>,
    item_edit_dialogs: forms::DialogFactory<forms::DialogItemEdit>,
    item_watch_dialogs: forms::InfoDialogFactory<forms::DialogItemWatch>,
    action_watch_dialogs: forms::InfoDialogFactory<forms::DialogActionWatch>,
    svc_call_dialogs: forms::InfoDialogFactory<forms::DialogSvcCall>,
    icon_stop: CppBox<QIcon>,
    icon_start: CppBox<QIcon>,
    icon_node: CppBox<QIcon>,
    icon_broker: CppBox<QIcon>,
    icon_items: CppBox<QIcon>,
    icon_log: CppBox<QIcon>,
    icon_services: CppBox<QIcon>,
    icon_spoints: CppBox<QIcon>,
    icon_action: CppBox<QIcon>,
    icon_users: CppBox<QIcon>,
    args: Args,
}

impl StaticUpcast<QObject> for Ui {
    unsafe fn static_upcast(ptr: Ptr<Self>) -> Ptr<QObject> {
        ptr.window.widget.as_ptr().static_upcast()
    }
}

unsafe fn qicon(id: &str) -> CppBox<QIcon> {
    let pixmap = QPixmap::new();
    pixmap.load_1a(&qs(format!(":/i/icons/{id}.png")));
    let icon = QIcon::new();
    icon.add_pixmap_1a(&pixmap);
    icon
}

impl Ui {
    pub fn new(args: Args) -> Rc<Self> {
        unsafe {
            let (cmd_tx, cmd_rx) = mpsc_std::sync_channel::<Command>(32768);
            let cleanup_timer: QBox<QTimer> = QTimer::new_0a();
            let window = forms::Main::load();
            let title = window.widget.window_title().to_std_string();
            let action_timer = QTimer::new_0a();
            action_timer.set_interval(100);
            action_timer.set_single_shot(true);
            let dialog_lvar_set = Rc::new(forms::DialogLvarSet::load());
            dialog_lvar_set.init();
            let dialog_unit_action = Rc::new(forms::DialogUnitAction::load());
            let this = Rc::new(Ui {
                window,
                dialog_connect: forms::DialogConnect::load(),
                dialog_lvar_set,
                dialog_unit_action,
                dialog_about: Rc::new(forms::DialogAbout::load()),
                dialog_export: Rc::new(forms::DialogExport::load()),
                busy: Rc::new(forms::Busy::load()),
                _source_model: QStandardItemModel::new_0a(),
                _proxy_model: QSortFilterProxyModel::new_0a(),
                cleanup_timer,
                cmd_rx,
                tree_items: <_>::default(),
                primary_table_items: <_>::default(),
                secondary_table_items: <_>::default(),
                auto_reload_timer: <_>::default(),
                auto_reload_auto_suspended: <_>::default(),
                title,
                config: <_>::default(),
                action_timer,
                svc_edit_dialogs: <_>::default(),
                item_edit_dialogs: <_>::default(),
                item_watch_dialogs: <_>::default(),
                action_watch_dialogs: <_>::default(),
                svc_call_dialogs: <_>::default(),
                icon_stop: qicon("stop"),
                icon_start: qicon("start"),
                icon_node: qicon("node"),
                icon_broker: qicon("broker"),
                icon_items: qicon("items"),
                icon_log: qicon("log"),
                icon_services: qicon("services"),
                icon_spoints: qicon("spoints"),
                icon_action: qicon("action"),
                icon_users: qicon("action"),
                args,
            });
            this.init(cmd_tx);
            this.dialog_export.init(&this.slot_on_export_clicked());
            this.cleanup_timer.timeout().connect(&this.slot_cleanup());
            this.cleanup_timer
                .start_1a(crate::UI_CLEANUP_INTERVAL.as_millis().try_into().unwrap());
            this.refire_auto_reload();
            this
        }
    }
    unsafe fn ui_action<F>(self: &Rc<Self>, mut action: F)
    where
        F: FnMut() -> EResult<String> + 'static,
    {
        let this = self.clone();
        self.action_timer.disconnect();
        self.action_timer
            .timeout()
            .connect(&SlotNoArgs::new(
                &self.window.widget,
                move || match action() {
                    Ok(result) => {
                        this.busy.mark_completed(&result);
                    }
                    Err(e) => {
                        this.busy.mark_failed(&e.to_string());
                    }
                },
            ));
        self.busy();
        self.action_timer.start_0a();
    }
    #[slot(SlotNoArgs)]
    unsafe fn handle_cmd(self: &Rc<Self>) {
        // move to events(?)
        if self.window.widget.is_visible() {
            if let Some(config) = self.config.lock().unwrap().as_mut() {
                config.set_auto_reload(self.window.auto_reload.value());
                let size = self.window.widget.size();
                config.set_main_window_size(size.width(), size.height());
                if let Some((x, y)) = splitter_sizes(&self.window.splitter_workspace) {
                    if x > 0 && y > 0 {
                        config.set_s_workspace(x, y);
                    }
                }
                if self.window.primary_table.is_visible()
                    && self.window.secondary_table.is_visible()
                {
                    if let Some((x, y)) = splitter_sizes(&self.window.splitter_tables) {
                        if x > 0 && y > 0 {
                            config.set_s_tables(x, y);
                        }
                    }
                }
            }
        }
        //
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                Command::ProcessItemWatch(u, data) => {
                    if !self.window.widget.is_visible()
                        || !self.item_watch_dialogs.push(u, Ok(data))
                    {
                        let _r = bus::call::<()>(Arc::new(NitData::stop_watcher(u)));
                    }
                }
                Command::ProcessSvcCallResult(u, data) => {
                    if !self.window.widget.is_visible() || !self.svc_call_dialogs.push(u, data) {
                        let _r = bus::call::<()>(Arc::new(NitData::stop_watcher(u)));
                    }
                }
                Command::ProcessActionWatch(u, data) => {
                    if !self.window.widget.is_visible()
                        || !self.action_watch_dialogs.push(u, Ok(data))
                    {
                        let _r = bus::call::<()>(Arc::new(NitData::stop_watcher(u)));
                    }
                }
                Command::SetStatus(v, kind) => match kind {
                    StatusKind::Info => {
                        self.window.set_status(&v);
                    }
                    StatusKind::Error => {
                        self.window.set_status(&format_err!(v));
                    }
                },
                Command::MarkConnected(path, node_list) => {
                    self.window.set_status(&format!("Connected: {}", path));
                    self.window
                        .widget
                        .set_window_title(&qs(format!("{} - {}", path, self.title)));
                    self.clear_workspace();
                    self.window.i_node.clear();
                    self.window.i_node.add_item_q_string(&qs("*"));
                    let mut tree_items = self.tree_items.lock().unwrap();
                    let mut first = true;
                    for node in node_list {
                        self.window.i_node.add_item_q_string(&qs(&node.name));
                        let mut item = NodeTreeItem::new(
                            &self.window.main_tree,
                            &node.name,
                            first,
                            &self.icon_node,
                        );
                        item.add("actions", &self.icon_action);
                        item.add("broker", &self.icon_broker);
                        item.add("items", &self.icon_items);
                        item.add("log", &self.icon_log);
                        item.add("services", &self.icon_services);
                        item.add("spoints", &self.icon_spoints);
                        item.add("users", &self.icon_users);
                        tree_items.insert(node.name, item);
                        first = false;
                    }
                    self.refire_auto_reload();
                }
                Command::MarkDisconnected => {
                    self.auto_reload_timer.lock().unwrap().take();
                    self.clear_workspace();
                    self.window.set_status("Disconnected");
                    self.window.set_nit_status("");
                    self.window.widget.set_window_title(&qs(&self.title));
                    self.item_watch_dialogs.close_all();
                    self.action_watch_dialogs.close_all();
                }
                Command::ProcessNit(nit) => {
                    self.process_nit(nit);
                }
            }
        }
    }
    unsafe fn busy(&self) {
        self.busy.show();
    }
    unsafe fn clear_workspace(&self) {
        self.tree_items.lock().unwrap().clear();
        self.primary_table_items.lock().unwrap().clear();
        self.secondary_table_items.lock().unwrap().clear();
        self.window.clear_workspace();
    }
    pub unsafe fn clear_tables(&self) {
        self.clear_primary_table();
        self.clear_secondary_table();
    }
    pub unsafe fn clear_primary_table(&self) {
        self.primary_table_items.lock().unwrap().clear();
        self.window.clear_primary_table();
    }
    pub unsafe fn clear_secondary_table(&self) {
        self.secondary_table_items.lock().unwrap().clear();
        self.window.clear_secondary_table();
    }
    unsafe fn init_splitters(self: &Rc<Self>) {
        if let Some((size_left, size_right)) = splitter_sizes(&self.window.splitter_workspace) {
            let size_total = size_left + size_right;
            let size_left = size_total / 3;
            let size_right = size_total / 3 * 2;
            self.window
                .splitter_workspace
                .set_sizes(&new_size(size_left, size_right));
        }
    }
    #[allow(clippy::too_many_lines)]
    unsafe fn init(self: &Rc<Self>, cmd_tx: mpsc_std::SyncSender<Command>) {
        let cmd_ch = crate::com_channel::ComChannel::new(cmd_tx);
        cmd_ch.signal().connect(&self.slot_handle_cmd());
        crate::UI_TX
            .set(Mutex::new(cmd_ch))
            .map_err(|_| Error::failed("FAILED TO SET UI TX"))
            .unwrap();
        self.clear_workspace();
        self.dialog_about.init();
        self.busy.init();
        #[cfg(debug_assertions)]
        {
            self.window.i_oid.set_text(&qs("*"));
        }
        let this = self.clone();
        let slot_reload = SlotNoArgs::new(&self.window.widget, move || {
            this.reload();
        });
        self.window
            .main_tree
            .item_selection_changed()
            .connect(&self.slot_on_main_tree_activated());
        self.window
            .main_tree
            .custom_context_menu_requested()
            .connect(&self.slot_on_tree_ctx());
        for table in [&self.window.primary_table, &self.window.secondary_table] {
            table
                .item_selection_changed()
                .connect(&self.slot_s_suspend_auto_reload());
            let header = table.horizontal_header();
            header
                .section_clicked()
                .connect(&self.slot_s_suspend_auto_reload());
        }
        self.window.i_oid.return_pressed().connect(&slot_reload);
        self.window.i_node.activated().connect(&slot_reload);
        self.window
            .primary_table
            .custom_context_menu_requested()
            .connect(&self.slot_on_primary_ctx());
        self.window
            .secondary_table
            .custom_context_menu_requested()
            .connect(&self.slot_on_secondary_ctx());
        //self.window
        //.btn_reload
        //.clicked()
        //.connect(&self.slot_on_button_clicked());
        //self.window
        //.btn_restart
        //.clicked()
        //.connect(&self.slot_on_button_restart());
        self.window
            .action_connect
            .triggered()
            .connect(&self.slot_on_action_connect());
        self.window
            .action_disconnect
            .triggered()
            .connect(&self.slot_on_action_disconnect());
        self.window.action_reload.triggered().connect(&slot_reload);
        self.window
            .action_exit
            .triggered()
            .connect(&SlotNoArgs::new(&self.window.widget, || {
                QApplication::close_all_windows();
            }));
        self.window
            .action_about
            .triggered()
            .connect(&self.slot_on_about());
        self.window
            .action_copy
            .triggered()
            .connect(&self.slot_on_copy());
        self.window
            .action_select_all
            .triggered()
            .connect(&self.slot_on_select_all());
        self.window
            .btn_auto_reload_start_stop
            .clicked()
            .connect(&self.slot_on_toggle_auto_reload());
        self.window
            .auto_reload
            .value_changed()
            .connect(&self.slot_on_auto_reload_changed());
        self.dialog_connect
            .button_box
            .accepted()
            .connect(&self.slot_on_connect_pressed());
        self.dialog_connect
            .proto
            .activated2()
            .connect(&self.slot_on_proto_selected());
        self.window
            .action_add_resource
            .triggered()
            .connect(&self.slot_on_add_resource());
        self.window
            .action_edit_resource
            .triggered()
            .connect(&self.slot_on_edit_resource());
        self.window
            .action_delete_resource
            .triggered()
            .connect(&self.slot_on_delete_resource());
        self.window
            .action_export_resource
            .triggered()
            .connect(&self.slot_on_export_resource());
        self.set_item_filter(false);
        self.set_log_filter(false);
        self.set_action_filter(false);
        self.set_user_filter(false);
        self.window.i_log_level.set_current_text(&qs("info"));
        self.window
            .action_import_resource
            .triggered()
            .connect(&self.slot_import_resource());
    }
    unsafe fn no_res(self: &Rc<Self>) {
        self.error_box(
            Some("Nothing to process"),
            "No resources to add/edit/delete in the current section",
        );
    }
    unsafe fn no_res_selected(self: &Rc<Self>) {
        self.error_box(
            Some("Resource not selected"),
            "Please select a resource in the primary table",
        );
    }
    unsafe fn svc_deploy(self: &Rc<Self>, dialog: Rc<forms::DialogSvcEdit>, node: &str) -> bool {
        match dialog.parse_params() {
            Ok(params) => {
                let nit = Arc::new(NitData::new_svc_deploy(node, params));
                match bus::call::<()>(nit) {
                    Ok(_) => true,
                    Err(e) => {
                        self.error_box(Some("Service deployment error"), e);
                        false
                    }
                }
            }
            Err(e) => {
                self.error_box(Some("Service params error"), e);
                false
            }
        }
    }
    unsafe fn item_deploy(self: &Rc<Self>, dialog: Rc<forms::DialogItemEdit>, node: &str) -> bool {
        match dialog.parse_config() {
            Ok(config) => {
                let nit = Arc::new(NitData::new_item_deploy(node, config));
                match bus::call::<()>(nit) {
                    Ok(_) => true,
                    Err(e) => {
                        self.error_box(Some("Item deployment error"), e);
                        false
                    }
                }
            }
            Err(e) => {
                self.error_box(Some("Item config error"), e);
                false
            }
        }
    }
    unsafe fn item_edit(self: &Rc<Self>, node: &str, oid: String) {
        match bus::call::<Value>(Arc::new(NitData::new_item_get_config_x(node, oid))) {
            Ok(val) => {
                if let Value::Seq(seq) = val {
                    if seq.len() == 2 {
                        let mut i = seq.into_iter();
                        if let Ok(config) = ItemConfig::deserialize(i.next().unwrap()) {
                            if let Ok(svcs) = Vec::deserialize(i.next().unwrap()) {
                                let dialog = Rc::new(forms::DialogItemEdit::load());
                                dialog.init();
                                let this = self.clone();
                                self.item_edit_dialogs.register(
                                    dialog.clone(),
                                    node,
                                    move |d, n| this.item_deploy(d, n),
                                );
                                dialog.show_edit(node, config, svcs);
                            }
                        }
                    }
                }
            }
            Err(e) => self.error("Failed to get service params", e),
        }
    }
    unsafe fn action_watch(self: &Rc<Self>, node: &str, action_uuid: uuid::Uuid) {
        let dialog = Rc::new(forms::DialogActionWatch::new(node, action_uuid));
        let u = self.action_watch_dialogs.register(dialog.clone());
        dialog.init(u);
        let _r = bus::call::<()>(Arc::new(NitData::start_action_watcher(
            u,
            node,
            action_uuid,
            Duration::from_secs(1),
        )));
        dialog.show();
    }
    unsafe fn item_watch(self: &Rc<Self>, node: &str, oid_str: String) {
        if let Ok(oid) = oid_str.parse::<OID>() {
            let dialog = Rc::new(forms::DialogItemWatch::new(node, &oid));
            let u = self.item_watch_dialogs.register(dialog.clone());
            dialog.init(u);
            let _r = bus::call::<()>(Arc::new(NitData::start_item_watcher(
                u,
                node,
                oid,
                Duration::from_secs(1),
            )));
            dialog.show();
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_add_resource(self: &Rc<Self>) {
        if let Some(nd) = self.current_nd() {
            match nd.kind() {
                NitKind::Services => {
                    let nit_sp = Arc::new(NitData::new_spoints(nd.node()));
                    if let Ok(spoints) = bus::call::<Vec<SPointInfo>>(nit_sp) {
                        let dialog = Rc::new(forms::DialogSvcEdit::load());
                        let ui_c = self.clone();
                        let dialog_c = dialog.clone();
                        let node_c = nd.node().to_owned();
                        let slot_load_tpl = SlotNoArgs::new(&dialog.widget, move || {
                            forms::on_svc_btn_load_clicked(&dialog_c, &node_c, &ui_c);
                        });
                        dialog.btn_load_tpl.clicked().connect(&slot_load_tpl);
                        let this = self.clone();
                        self.svc_edit_dialogs
                            .register(dialog.clone(), nd.node(), move |d, n| this.svc_deploy(d, n));
                        dialog.show_add(nd.node(), spoints);
                    } else {
                        self.default_error_box(ERR_LOAD_SPOINTS);
                    }
                }
                NitKind::Items(_, _) => {
                    let nit_sp = Arc::new(NitData::new_services(nd.node()));
                    if let Ok(svcs) = bus::call::<Vec<SvcData>>(nit_sp) {
                        let dialog = Rc::new(forms::DialogItemEdit::load());
                        dialog.init();
                        let this = self.clone();
                        self.item_edit_dialogs
                            .register(dialog.clone(), nd.node(), move |d, n| {
                                this.item_deploy(d, n)
                            });
                        dialog.show_add(nd.node(), svcs);
                    } else {
                        self.default_error_box(ERR_LOAD_SPOINTS);
                    }
                }
                _ => {
                    self.no_res();
                }
            }
        } else {
            self.no_res();
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_edit_resource(self: &Rc<Self>) {
        if let Some(nd) = self.current_nd() {
            let table = &self.window.primary_table;
            let current_item = table.current_item();
            if current_item.is_null() {
                self.no_res_selected();
            } else {
                let row = current_item.row();
                let current_res_id = table.item(row, 0).text().to_std_string();
                match nd.kind() {
                    NitKind::Services => {
                        self.svc_edit(nd.node(), current_res_id);
                    }
                    NitKind::Items(_, _) => {
                        self.item_edit(nd.node(), current_res_id);
                    }
                    _ => {
                        self.no_res();
                    }
                }
            }
        } else {
            self.no_res();
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_delete_resource(self: &Rc<Self>) {
        if let Some(nd) = self.current_nd() {
            let table = &self.window.primary_table;
            if let Some(resources) = table.selected_resources() {
                match nd.kind() {
                    NitKind::Services => {
                        self.svc_destroy(nd.node(), resources);
                    }
                    NitKind::Items(_, _) => {
                        self.item_destroy(nd.node(), resources);
                    }
                    _ => {
                        self.no_res();
                    }
                }
            } else {
                self.no_res_selected();
            }
        } else {
            self.no_res();
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_export_resource(self: &Rc<Self>) {
        if let Some(nd) = self.current_nd() {
            let table = &self.window.primary_table;
            if table.selected_resources().is_some() {
                match nd.kind() {
                    NitKind::Services => {
                        self.dialog_export.show(nd.node(), KIND_SVC);
                    }
                    NitKind::Items(_, _) => {
                        self.dialog_export.show(nd.node(), KIND_ITEM);
                    }
                    _ => {
                        self.no_res();
                    }
                }
            } else {
                self.no_res_selected();
            }
        } else {
            self.no_res();
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn import_resource(self: &Rc<Self>) {
        if let Some(nd) = self.current_nd() {
            match nd.kind() {
                NitKind::Services | NitKind::Items(_, _) => {
                    let fname = QFileDialog::get_open_file_name_4a(
                        &self.window.widget,
                        &qs(forms::IN_FILE),
                        &qs(forms::get_last_dir()),
                        &qs(forms::YAML_FILTER),
                    )
                    .to_std_string();
                    if fname.is_empty() {
                        return;
                    }
                    self.ui_action(move || {
                        let mut data: HashMap<String, Value> =
                            load_yaml(&fname)?.ok_or_else(|| Error::invalid_data("No input"))?;
                        if data.contains_key("version") {
                            return Err(Error::invalid_data(
                                "cloud deploy files can not be imported",
                            ));
                        }
                        let (nd_import, kind, cnt) = match nd.kind() {
                            NitKind::Services => {
                                if let Some(svcs) = data.remove("svcs") {
                                    if let Value::Seq(seq) = svcs {
                                        let cnt = seq.len();
                                        (
                                            NitData::new_svc_deploy_multi(nd.node(), seq),
                                            KIND_SVC,
                                            cnt,
                                        )
                                    } else {
                                        return Err(Error::invalid_data("section is not a seq"));
                                    }
                                } else {
                                    return Err(Error::invalid_data("no svcs section"));
                                }
                            }
                            NitKind::Items(_, _) => {
                                if let Some(items) = data.remove("items") {
                                    if let Value::Seq(seq) = items {
                                        let cnt = seq.len();
                                        (
                                            NitData::new_item_deploy_multi(nd.node(), seq),
                                            KIND_ITEM,
                                            cnt,
                                        )
                                    } else {
                                        return Err(Error::invalid_data("section is not a seq"));
                                    }
                                } else {
                                    return Err(Error::invalid_data("no items section"));
                                }
                            }
                            _ => {
                                return Err(Error::unsupported("import kind"));
                            }
                        };
                        bus::call::<()>(Arc::new(nd_import))?;
                        Ok(format!("{cnt} {kind} imported"))
                    });
                }
                _ => self.no_res(),
            }
        } else {
            self.no_res();
        }
    }
    #[slot(SlotNoArgs)]
    #[allow(clippy::too_many_lines)]
    unsafe fn on_export_clicked(self: &Rc<Self>) {
        macro_rules! abort {
            ($title: expr, $msg: expr) => {
                self.error_box($title, $msg);
                self.dialog_export.show0();
                return;
            };
        }
        if let Some(nd) = self.current_nd() {
            let table = &self.window.primary_table;
            if let Some(resources) = table.selected_resources() {
                let res_count = resources.len();
                let export_config = self.dialog_export.export_config();
                if let Some(fname) = export_config.file {
                    if export_config.kind == ExportKind::CloudDeploy
                        && export_config.cloud_node.is_none()
                    {
                        abort!(
                            Some("Node not specified"),
                            format!(
                                "Cloud deploy export selected but the target node is not specified"
                            )
                        );
                    }
                    macro_rules! create_map {
                        () => {{
                            let mut map = BTreeMap::new();
                            if export_config.kind == ExportKind::CloudDeploy {
                                map.insert("version".into(), Value::U8(4));
                            }
                            map
                        }};
                    }
                    let mut data: BTreeMap<Value, Value> = if export_config.merge {
                        match load_yaml(&fname) {
                            Ok(Some(v)) => {
                                let version_key: Value = "version".into();
                                let map: &BTreeMap<Value, Value> = &v;
                                match export_config.kind {
                                    ExportKind::Resource => {
                                        if map.contains_key(&version_key) {
                                            abort!(None::<&str>,
                                                "Attempt to export as resources into cloud deploy file");
                                        }
                                    }
                                    ExportKind::CloudDeploy => {
                                        if !map.contains_key(&version_key) {
                                            abort!(None::<&str>,
                                                "Attempt to export as cloud deploy into resource file");
                                        }
                                    }
                                }
                                v
                            }
                            Ok(None) => {
                                create_map!()
                            }
                            Err(e) => {
                                abort!(
                                    Some("Output file error"),
                                    format!("Output file parse error: {e}")
                                );
                            }
                        }
                    } else {
                        create_map!()
                    };
                    self.ui_action(move || {
                        let x_kind = match nd.kind() {
                            NitKind::Services => {
                                let svcs = find_export_entry(
                                    &mut data,
                                    "svcs",
                                    export_config.kind == ExportKind::CloudDeploy,
                                    export_config.cloud_node.as_deref(),
                                )?;
                                for res in &resources {
                                    let nit_cfg = Arc::new(NitData::new_svc_get_params(
                                        nd.node(),
                                        res.clone(),
                                    ));
                                    let svc_config = bus::call::<Value>(nit_cfg)?;
                                    let id_key = Value::String("id".to_owned());
                                    let mut svc_map = None;
                                    for svc in svcs.iter_mut() {
                                        if let Value::Map(m) = svc {
                                            if let Some(id) = m.get(&id_key) {
                                                if &id.to_string() == res {
                                                    svc_map = Some(m);
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    if let Some(map) = svc_map {
                                        map.insert("params".into(), svc_config);
                                    } else {
                                        let mut map: BTreeMap<Value, Value> = BTreeMap::new();
                                        map.insert("id".into(), Value::String(res.clone()));
                                        map.insert("params".into(), svc_config);
                                        svcs.push(Value::Map(map));
                                    }
                                }
                                KIND_SVC
                            }
                            NitKind::Items(_, _) => {
                                let items = find_export_entry(
                                    &mut data,
                                    "items",
                                    export_config.kind == ExportKind::CloudDeploy,
                                    export_config.cloud_node.as_deref(),
                                )?;
                                for res in &resources {
                                    let nit_cfg = Arc::new(NitData::new_item_get_config(
                                        nd.node(),
                                        res.clone(),
                                    ));
                                    let item_config = bus::call::<Value>(nit_cfg)?;
                                    let oid_key = Value::String("oid".to_owned());
                                    items.retain(|item| {
                                        if let Value::Map(m) = item {
                                            if let Some(oid) = m.get(&oid_key) {
                                                &oid.to_string() != res
                                            } else {
                                                true
                                            }
                                        } else {
                                            true
                                        }
                                    });
                                    items.push(item_config);
                                }
                                KIND_ITEM
                            }
                            _ => {
                                return Err(Error::not_implemented("export kind"));
                            }
                        };
                        save_yaml(&fname, &data)?;
                        Ok(format!("{res_count} {x_kind} exported"))
                    });
                } else {
                    abort!(Some("Failed"), "Output file not specified");
                }
            }
        }
    }
    unsafe fn ctx_nodes(self: &Rc<Self>, node: &str, pos: CppBox<QPoint>) {
        const CA_SAVE: &str = "node_ca_save";
        const CA_RESTART: &str = "node_ca_restart";
        let menu = QMenu::new();
        let action_save = QAction::new();
        action_save.set_object_name(&qs(CA_SAVE));
        action_save.set_text(&qs("&Save"));
        menu.add_action(&action_save);
        let action_restart = QAction::new();
        action_restart.set_object_name(&qs(CA_RESTART));
        action_restart.set_text(&qs("&Restart"));
        menu.add_action(&action_restart);
        let selected = menu.exec_1a_mut(&pos);
        if selected.is_null() {
            return;
        }
        match selected.object_name().to_std_string().as_str() {
            CA_SAVE => {
                self.process_action_nit(Arc::new(NitData::new_save(node)));
            }
            CA_RESTART => {
                if self.confirm(&format!(
                    r#"The node <b>{}</b> is going to be RESTARTED.<br>
If this is the node Cloud Manager is connected to, the session will be disconnected."#,
                    node
                )) {
                    self.process_action_nit(Arc::new(NitData::new_restart(node)));
                }
            }
            _ => {}
        }
    }
    unsafe fn confirm_obj_action(self: &Rc<Self>, kind: &str, op: &str, which: &[String]) -> bool {
        let mut w = which
            .iter()
            .take(MAX_CONFIRM)
            .map(String::as_str)
            .collect::<Vec<&str>>()
            .join("<br>");
        if which.len() > MAX_CONFIRM {
            let _r = write!(w, "<br>...and {} more", which.len() - MAX_CONFIRM);
        }
        self.confirm(&format!(
            "The following {} will be {}:<br><br>{}",
            kind, op, w
        ))
    }
    unsafe fn svc_destroy(self: &Rc<Self>, node: &str, svcs: Vec<String>) {
        if self.confirm_obj_action(KIND_SVC, "DESTROYED", &svcs) {
            self.process_action_nit(Arc::new(NitData::new_svc_destroy(node, svcs)));
        }
    }
    unsafe fn item_announce(self: &Rc<Self>, node: &str, oids: Vec<String>) {
        self.process_action_nit(Arc::new(NitData::new_item_announce(node, oids)));
    }
    unsafe fn item_destroy(self: &Rc<Self>, node: &str, oids: Vec<String>) {
        if self.confirm_obj_action(KIND_ITEM, "DESTROYED", &oids) {
            self.process_action_nit(Arc::new(NitData::new_item_destroy(node, oids)));
        }
    }
    unsafe fn item_disable(self: &Rc<Self>, node: &str, oids: Vec<String>) {
        if self.confirm_obj_action(KIND_ITEM, "DISABLED", &oids) {
            self.process_action_nit(Arc::new(NitData::new_item_disable(node, oids)));
        }
    }
    unsafe fn item_enable(self: &Rc<Self>, node: &str, oids: Vec<String>) {
        if self.confirm_obj_action(KIND_ITEM, "ENABLED", &oids) {
            self.process_action_nit(Arc::new(NitData::new_item_enable(node, oids)));
        }
    }
    unsafe fn lmacro_run(self: &Rc<Self>, node: &str, current_eva_item: String) {
        let dialog_lmacro_run = Rc::new(forms::DialogLmacroRun::new());
        dialog_lmacro_run.qdialog.btn_box.disconnect();
        dialog_lmacro_run
            .qdialog
            .widget
            .set_window_title(&qs(format!("{} run", current_eva_item)));
        let node = node.to_owned();
        let this = self.clone();
        let dialog = dialog_lmacro_run.clone();
        dialog_lmacro_run
            .qdialog
            .btn_box
            .accepted()
            .connect(&SlotNoArgs::new(
                &dialog_lmacro_run.qdialog.widget,
                move || match dialog.parse_payload() {
                    Ok(mut p_lmacro_run) => {
                        dialog.qdialog.widget.close();
                        if this.confirm_obj_action(
                            KIND_LMACRO,
                            "EXECUTED",
                            &[current_eva_item.clone()],
                        ) {
                            p_lmacro_run.i.replace(current_eva_item.clone());
                            match bus::call::<ActionRecord>(Arc::new(NitData::new_lmacro_run(
                                &node,
                                p_lmacro_run,
                            ))) {
                                Ok(a) => {
                                    this.action_watch(&node, a.uuid);
                                }
                                Err(e) => {
                                    this.error_box(None::<&str>, e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        this.error_box(None::<&str>, e);
                    }
                },
            ));
        let dialog = dialog_lmacro_run.clone();
        dialog_lmacro_run
            .qdialog
            .btn_box
            .rejected()
            .connect(&SlotNoArgs::new(
                &dialog_lmacro_run.qdialog.widget,
                move || {
                    dialog.qdialog.widget.close();
                },
            ));
        dialog_lmacro_run.show();
    }
    unsafe fn unit_action(self: &Rc<Self>, node: &str, current_eva_item: String) {
        let state = if let Ok(oid) = current_eva_item.parse::<OID>() {
            bus::item_state(node, oid).ok()
        } else {
            None
        };
        self.dialog_unit_action.btn_box.disconnect();
        self.dialog_unit_action
            .widget
            .set_window_title(&qs(format!("{} action", current_eva_item)));
        let node = node.to_owned();
        let this = self.clone();
        self.dialog_unit_action
            .btn_box
            .accepted()
            .connect(&SlotNoArgs::new(
                &self.dialog_unit_action.widget,
                move || match this.dialog_unit_action.parse_payload() {
                    Ok(mut p_action) => {
                        this.dialog_unit_action.widget.close();
                        if this.confirm_obj_action(
                            KIND_UNIT,
                            "ALTERTED WITH ACTION",
                            &[current_eva_item.clone()],
                        ) {
                            p_action.i.replace(current_eva_item.clone());
                            match bus::call::<ActionRecord>(Arc::new(NitData::new_unit_action(
                                &node, p_action,
                            ))) {
                                Ok(a) => {
                                    this.action_watch(&node, a.uuid);
                                }
                                Err(e) => {
                                    this.error_box(None::<&str>, e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        this.error_box(None::<&str>, e);
                    }
                },
            ));
        let this = self.clone();
        self.dialog_unit_action
            .btn_box
            .rejected()
            .connect(&SlotNoArgs::new(
                &self.dialog_unit_action.widget,
                move || {
                    this.dialog_unit_action.widget.close();
                },
            ));
        self.dialog_unit_action.show(state);
    }
    unsafe fn unit_action_toggle(self: &Rc<Self>, node: &str, oid: String) {
        if self.confirm_obj_action(KIND_UNIT, "ALTERED WITH ACTION-TOGGLE", &[oid.clone()]) {
            match bus::call::<ActionRecord>(Arc::new(NitData::new_unit_action_toggle(node, oid))) {
                Ok(a) => {
                    self.action_watch(node, a.uuid);
                }
                Err(e) => {
                    self.error_box(None::<&str>, e);
                }
            }
        }
    }
    unsafe fn lvar_set(self: &Rc<Self>, node: &str, current_eva_item: &str, oids: Vec<String>) {
        let state = if let Ok(oid) = current_eva_item.parse::<OID>() {
            bus::item_state(node, oid).ok()
        } else {
            None
        };
        self.dialog_lvar_set.btn_box.disconnect();
        let node = node.to_owned();
        let this = self.clone();
        self.dialog_lvar_set
            .btn_box
            .accepted()
            .connect(&SlotNoArgs::new(
                &self.dialog_lvar_set.widget,
                move || match this.dialog_lvar_set.parse_payload() {
                    Ok(p_set) => {
                        this.dialog_lvar_set.widget.close();
                        if this.confirm_obj_action(KIND_LVAR, "SET", &oids) {
                            this.process_action_nit(Arc::new(NitData::new_lvar_set(
                                &node,
                                oids.clone(),
                                p_set,
                            )));
                        }
                    }
                    Err(e) => {
                        this.error_box(None::<&str>, e);
                    }
                },
            ));
        let this = self.clone();
        self.dialog_lvar_set
            .btn_box
            .rejected()
            .connect(&SlotNoArgs::new(&self.dialog_lvar_set.widget, move || {
                this.dialog_lvar_set.widget.close();
            }));
        self.dialog_lvar_set.show(state);
    }
    unsafe fn lvar_reset(self: &Rc<Self>, node: &str, oids: Vec<String>) {
        if self.confirm_obj_action(KIND_LVAR, "RESETED", &oids) {
            self.process_action_nit(Arc::new(NitData::new_lvar_reset(node, oids)));
        }
    }
    unsafe fn lvar_clear(self: &Rc<Self>, node: &str, oids: Vec<String>) {
        if self.confirm_obj_action(KIND_LVAR, "CLEARED", &oids) {
            self.process_action_nit(Arc::new(NitData::new_lvar_clear(node, oids)));
        }
    }
    unsafe fn lvar_toggle(self: &Rc<Self>, node: &str, oids: Vec<String>) {
        if self.confirm_obj_action(KIND_LVAR, "TOGGLED", &oids) {
            self.process_action_nit(Arc::new(NitData::new_lvar_toggle(node, oids)));
        }
    }
    unsafe fn lvar_incr(self: &Rc<Self>, node: &str, oids: Vec<String>) {
        if self.confirm_obj_action(KIND_LVAR, "INCREMENTED", &oids) {
            self.process_action_nit(Arc::new(NitData::new_lvar_incr(node, oids)));
        }
    }
    unsafe fn lvar_decr(self: &Rc<Self>, node: &str, oids: Vec<String>) {
        if self.confirm_obj_action(KIND_LVAR, "DECREMENTED", &oids) {
            self.process_action_nit(Arc::new(NitData::new_lvar_decr(node, oids)));
        }
    }
    unsafe fn svc_edit(self: &Rc<Self>, node: &str, svc: String) {
        match bus::call::<Value>(Arc::new(NitData::new_svc_get_params_x(node, svc.clone()))) {
            Ok(val) => {
                if let Value::Seq(seq) = val {
                    if seq.len() == 2 {
                        let mut i = seq.into_iter();
                        if let Ok(mut params) = ServiceParams::deserialize(i.next().unwrap()) {
                            if let Ok(spoints) = Vec::deserialize(i.next().unwrap()) {
                                params.id.replace(svc);
                                let dialog = Rc::new(forms::DialogSvcEdit::load());
                                let this = self.clone();
                                self.svc_edit_dialogs.register(
                                    dialog.clone(),
                                    node,
                                    move |d, n| this.svc_deploy(d, n),
                                );
                                dialog.show_edit(node, params, spoints);
                            }
                        }
                    }
                }
            }
            Err(e) => self.error("Failed to get service params", e),
        }
    }
    unsafe fn svc_call_method(self: &Rc<Self>, node: &str, svc: String) {
        match bus::call::<Value>(Arc::new(NitData::new_svc_get_info(node, svc.clone()))) {
            Ok(val) => match SvcInfo::deserialize(val) {
                Ok(info) => {
                    let dialog = forms::DialogSvcCall::new(&svc, node, info);
                    dialog.show();
                    let d = dialog.clone();
                    let u = self.svc_call_dialogs.register(dialog);
                    d.set_uuid(u);
                }
                Err(e) => self.error("Failed to parse service info", e),
            },
            Err(e) => self.error("Failed to get service info", e),
        }
    }
    unsafe fn ctx_svcs(
        self: &Rc<Self>,
        svcs: Vec<String>,
        current_svc: String,
        pos: CppBox<QPoint>,
        node: &str,
    ) {
        const CA_CALL: &str = "svc_ca_call";
        const CA_EDIT: &str = "svc_ca_edit";
        const CA_EXPORT: &str = "svc_ca_export";
        const CA_IMPORT: &str = "svc_ca_import";
        const CA_RESTART: &str = "svc_ca_restart";
        const CA_DESTROY: &str = "svc_ca_destroy";
        const CA_PURGE: &str = "svc_ca_purge";
        let menu = QMenu::new();
        let action_call = QAction::new();
        action_call.set_object_name(&qs(CA_CALL));
        action_call.set_text(&qs("&Call a method"));
        menu.add_action(&action_call);
        let action_edit = QAction::new();
        action_edit.set_object_name(&qs(CA_EDIT));
        action_edit.set_text(&qs("&Edit"));
        menu.add_action(&action_edit);
        let action_export = QAction::new();
        action_export.set_object_name(&qs(CA_EXPORT));
        action_export.set_text(&qs("E&xport"));
        menu.add_action(&action_export);
        let action_import = QAction::new();
        action_import.set_object_name(&qs(CA_IMPORT));
        action_import.set_text(&qs("&Import"));
        menu.add_action(&action_import);
        let action_restart = QAction::new();
        action_restart.set_object_name(&qs(CA_RESTART));
        action_restart.set_text(&qs("&Restart"));
        menu.add_action(&action_restart);
        menu.add_separator();
        let action_destroy = QAction::new();
        action_destroy.set_object_name(&qs(CA_DESTROY));
        action_destroy.set_text(&qs("&Destroy"));
        menu.add_action(&action_destroy);
        let action_purge = QAction::new();
        action_purge.set_object_name(&qs(CA_PURGE));
        action_purge.set_text(&qs("&Purge"));
        menu.add_action(&action_purge);
        let selected = menu.exec_1a_mut(&pos);
        if selected.is_null() {
            return;
        }
        match selected.object_name().to_std_string().as_str() {
            CA_CALL => {
                self.svc_call_method(node, current_svc);
            }
            CA_EDIT => {
                self.svc_edit(node, current_svc);
            }
            CA_EXPORT => {
                self.dialog_export.show(node, KIND_SVC);
            }
            CA_IMPORT => {
                self.import_resource();
            }
            CA_RESTART => {
                if self.confirm_obj_action(KIND_SVC, "RESTARTED", &svcs) {
                    self.process_action_nit(Arc::new(NitData::new_svc_restart(node, svcs)));
                }
            }
            CA_DESTROY => {
                self.svc_destroy(node, svcs);
            }
            CA_PURGE => {
                if self.confirm_obj_action(KIND_SVC, "PURGED", &svcs) {
                    self.process_action_nit(Arc::new(NitData::new_svc_purge(node, svcs)));
                }
            }
            _ => {}
        }
    }
    unsafe fn ctx_actions(
        self: &Rc<Self>,
        _items: Vec<String>,
        current_action: String,
        pos: CppBox<QPoint>,
        node: &str,
    ) {
        const CA_WATCH: &str = "action_ca_watch";
        let menu = QMenu::new();
        let action_watch = QAction::new();
        action_watch.set_object_name(&qs(CA_WATCH));
        action_watch.set_text(&qs("&Watch"));
        menu.add_action(&action_watch);
        let selected = menu.exec_1a_mut(&pos);
        if selected.is_null() {
            return;
        }
        match selected.object_name().to_std_string().as_str() {
            CA_WATCH => match current_action.parse::<uuid::Uuid>() {
                Ok(u) => {
                    self.action_watch(node, u);
                }
                Err(e) => {
                    eprintln!("{}", e);
                }
            },
            _ => {}
        }
    }
    #[allow(clippy::too_many_lines)]
    unsafe fn ctx_items(
        self: &Rc<Self>,
        items: Vec<String>,
        current_eva_item: String,
        pos: CppBox<QPoint>,
        node: &str,
    ) {
        const CA_ANNOUNCE: &str = "item_ca_announce";
        const CA_EDIT: &str = "item_ca_edit";
        const CA_EXPORT: &str = "item_ca_export";
        const CA_IMPORT: &str = "item_ca_import";
        const CA_DISABLE: &str = "item_ca_disable";
        const CA_ENABLE: &str = "item_ca_enable";
        const CA_DESTROY: &str = "item_ca_destroy";
        const CA_WATCH: &str = "item_ca_watch";
        const CA_FILTER: &str = "item_ca_filter_";
        //const CA_SET_ITEM_STATE: &str = "item_ca_state_set";
        const CA_LMACRO_RUN: &str = "item_ca_lmacro_run";
        const CA_UNIT_ACTION: &str = "item_ca_unit_action";
        const CA_UNIT_ACTION_TOGGLE: &str = "item_ca_unit_action_toggle";
        const CA_LVAR_SET: &str = "item_ca_lvar_set";
        const CA_LVAR_RESET: &str = "item_ca_lvar_reset";
        const CA_LVAR_CLEAR: &str = "item_ca_lvar_clear";
        const CA_LVAR_TOGGLE: &str = "item_ca_lvar_toggle";
        const CA_LVAR_INCR: &str = "item_ca_lvar_incr";
        const CA_LVAR_DECR: &str = "item_ca_lvar_decr";
        let menu = QMenu::new();
        let mut secondary_actions = Vec::new();
        let mut x_actions = Vec::new();
        let _item_menu = if let Ok(oid) = current_eva_item.parse::<OID>() {
            let item_menu = QMenu::new();
            item_menu.set_title(&qs("&Filter by"));
            let action_filter_by_kind = QAction::new();
            let filter = format!("{}:#", oid.kind());
            action_filter_by_kind.set_object_name(&qs(format!("{}{}", CA_FILTER, filter)));
            action_filter_by_kind.set_text(&qs(format!("&{}", filter)));
            item_menu.add_action(&action_filter_by_kind);
            secondary_actions.push(action_filter_by_kind);
            if let Some(group) = oid.group() {
                macro_rules! add_filter_group {
                    ($grp: expr) => {
                        let action_filter_by_group = QAction::new();
                        let filter = format!("{}:{}/#", oid.kind(), $grp);
                        action_filter_by_group
                            .set_object_name(&qs(format!("{}{}", CA_FILTER, filter)));
                        action_filter_by_group.set_text(&qs(format!("{}", filter)));
                        item_menu.add_action(&action_filter_by_group);
                        secondary_actions.push(action_filter_by_group);
                    };
                }
                for (pos, _) in group.match_indices('/') {
                    add_filter_group!(&group[..pos]);
                }
                add_filter_group!(group);
            }
            menu.add_menu_q_menu(&item_menu);
            menu.add_separator();
            if oid.kind() == ItemKind::Unit
                || oid.kind() == ItemKind::Sensor
                || oid.kind() == ItemKind::Lvar
            {
                let action_watch = QAction::new();
                action_watch.set_object_name(&qs(CA_WATCH));
                action_watch.set_text(&qs("&Watch"));
                menu.add_action(&action_watch);
                x_actions.push(action_watch);
            };
            match oid.kind() {
                ItemKind::Unit => {
                    let action_action = QAction::new();
                    action_action.set_object_name(&qs(CA_UNIT_ACTION));
                    action_action.set_text(&qs("&Action"));
                    menu.add_action(&action_action);
                    x_actions.push(action_action);
                    let action_toggle = QAction::new();
                    action_toggle.set_object_name(&qs(CA_UNIT_ACTION_TOGGLE));
                    action_toggle.set_text(&qs("Action &toggle"));
                    menu.add_action(&action_toggle);
                    x_actions.push(action_toggle);
                }
                ItemKind::Lmacro => {
                    let action_run = QAction::new();
                    action_run.set_object_name(&qs(CA_LMACRO_RUN));
                    action_run.set_text(&qs("&Run"));
                    menu.add_action(&action_run);
                    x_actions.push(action_run);
                }
                _ => {}
            }
            Some(item_menu)
        } else {
            None
        };
        let mut items_with_state = Vec::new();
        let mut lvar_items = Vec::new();
        for i in &items {
            if let Ok(oid) = i.parse::<OID>() {
                if oid.kind() == ItemKind::Lvar {
                    lvar_items.push(i);
                }
                if oid.kind() == ItemKind::Unit
                    || oid.kind() == ItemKind::Sensor
                    || oid.kind() == ItemKind::Lvar
                {
                    items_with_state.push(i);
                }
            }
        }
        let _lvars_menu = if lvar_items.is_empty() {
            None
        } else {
            let lvars_menu = QMenu::new();
            lvars_menu.set_title(&qs("&Lvar ops"));
            menu.add_menu_q_menu(&lvars_menu);
            let action_lvar_set = QAction::new();
            action_lvar_set.set_object_name(&qs(CA_LVAR_SET));
            action_lvar_set.set_text(&qs("&Set"));
            lvars_menu.add_action(&action_lvar_set);
            x_actions.push(action_lvar_set);
            let action_lvar_reset = QAction::new();
            action_lvar_reset.set_object_name(&qs(CA_LVAR_RESET));
            action_lvar_reset.set_text(&qs("&Reset"));
            lvars_menu.add_action(&action_lvar_reset);
            x_actions.push(action_lvar_reset);
            let action_lvar_clear = QAction::new();
            action_lvar_clear.set_object_name(&qs(CA_LVAR_CLEAR));
            action_lvar_clear.set_text(&qs("&Clear"));
            lvars_menu.add_action(&action_lvar_clear);
            x_actions.push(action_lvar_clear);
            let action_lvar_toggle = QAction::new();
            action_lvar_toggle.set_object_name(&qs(CA_LVAR_TOGGLE));
            action_lvar_toggle.set_text(&qs("&Toggle"));
            lvars_menu.add_action(&action_lvar_toggle);
            x_actions.push(action_lvar_toggle);
            let action_lvar_incr = QAction::new();
            action_lvar_incr.set_object_name(&qs(CA_LVAR_INCR));
            action_lvar_incr.set_text(&qs("&Increment"));
            lvars_menu.add_action(&action_lvar_incr);
            x_actions.push(action_lvar_incr);
            let action_lvar_decr = QAction::new();
            action_lvar_decr.set_object_name(&qs(CA_LVAR_DECR));
            action_lvar_decr.set_text(&qs("&Decrement"));
            lvars_menu.add_action(&action_lvar_decr);
            x_actions.push(action_lvar_decr);
            Some(lvars_menu)
        };
        if !items_with_state.is_empty() {
            //let action_item_set = QAction::new();
            //action_item_set.set_object_name(&qs(CA_SET_ITEM_STATE));
            //action_item_set.set_text(&qs("&Set state"));
            //menu.add_action(&action_item_set);
            //x_actions.push(action_item_set);
            let action_announce = QAction::new();
            action_announce.set_object_name(&qs(CA_ANNOUNCE));
            action_announce.set_text(&qs("A&nnounce"));
            menu.add_action(&action_announce);
            x_actions.push(action_announce);
        }
        if !items_with_state.is_empty() || !lvar_items.is_empty() {
            menu.add_separator();
        }
        let action_edit = QAction::new();
        action_edit.set_object_name(&qs(CA_EDIT));
        action_edit.set_text(&qs("&Edit"));
        menu.add_action(&action_edit);
        let action_export = QAction::new();
        action_export.set_object_name(&qs(CA_EXPORT));
        action_export.set_text(&qs("E&xport"));
        menu.add_action(&action_export);
        let action_import = QAction::new();
        action_import.set_object_name(&qs(CA_IMPORT));
        action_import.set_text(&qs("&Import"));
        menu.add_action(&action_import);
        let action_disable = QAction::new();
        action_disable.set_object_name(&qs(CA_DISABLE));
        action_disable.set_text(&qs("Disa&ble"));
        menu.add_action(&action_disable);
        let action_enable = QAction::new();
        action_enable.set_object_name(&qs(CA_ENABLE));
        action_enable.set_text(&qs("E&nable"));
        menu.add_action(&action_enable);
        menu.add_separator();
        let action_destroy = QAction::new();
        action_destroy.set_object_name(&qs(CA_DESTROY));
        action_destroy.set_text(&qs("&Destroy"));
        menu.add_action(&action_destroy);
        let selected = menu.exec_1a_mut(&pos);
        if selected.is_null() {
            return;
        }
        match selected.object_name().to_std_string().as_str() {
            CA_ANNOUNCE => {
                self.item_announce(node, items);
            }
            CA_EDIT => {
                self.item_edit(node, current_eva_item);
            }
            CA_EXPORT => {
                self.dialog_export.show(node, KIND_ITEM);
            }
            CA_WATCH => {
                self.item_watch(node, current_eva_item);
            }
            CA_IMPORT => {
                self.import_resource();
            }
            CA_DISABLE => {
                self.item_disable(node, items);
            }
            CA_ENABLE => {
                self.item_enable(node, items);
            }
            CA_DESTROY => {
                self.item_destroy(node, items);
            }
            CA_LVAR_SET => {
                self.lvar_set(
                    node,
                    &current_eva_item,
                    lvar_items.into_iter().cloned().collect(),
                );
            }
            CA_LVAR_RESET => {
                self.lvar_reset(node, lvar_items.into_iter().cloned().collect());
            }
            CA_LVAR_CLEAR => {
                self.lvar_clear(node, lvar_items.into_iter().cloned().collect());
            }
            CA_LVAR_TOGGLE => {
                self.lvar_toggle(node, lvar_items.into_iter().cloned().collect());
            }
            CA_LVAR_INCR => {
                self.lvar_incr(node, lvar_items.into_iter().cloned().collect());
            }
            CA_LVAR_DECR => {
                self.lvar_decr(node, lvar_items.into_iter().cloned().collect());
            }
            CA_LMACRO_RUN => {
                self.lmacro_run(node, current_eva_item);
            }
            CA_UNIT_ACTION => {
                self.unit_action(node, current_eva_item);
            }
            CA_UNIT_ACTION_TOGGLE => {
                self.unit_action_toggle(node, current_eva_item);
            }
            n => {
                if let Some(oid_filter) = n.strip_prefix(CA_FILTER) {
                    self.window.i_oid.set_text(&qs(oid_filter));
                    self.reload();
                }
            }
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_primary_ctx(self: &Rc<Self>) {
        let table = &self.window.primary_table;
        let current_item = table.current_item();
        if !current_item.is_null() {
            if let Some(nd) = self.current_nd() {
                match nd.kind() {
                    NitKind::Services => {
                        self.svc_list_ctx(table, current_item, nd.node());
                    }
                    NitKind::Actions(_) => {
                        self.action_list_ctx(table, current_item, nd.node());
                    }
                    NitKind::Items(_, _) => {
                        self.item_list_ctx(table, current_item, nd.node());
                    }
                    _ => {}
                }
            } else {
                // no tree selection means local state table
                self.node_list_ctx(table, current_item);
            }
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_secondary_ctx(self: &Rc<Self>) {
        let table = &self.window.secondary_table;
        let current_item = table.current_item();
        if !current_item.is_null() {
            if let Some(nit) = self.current_nd() {
                match nit.kind() {
                    NitKind::State => {
                        self.node_list_ctx(table, current_item);
                    }
                    _ => {}
                }
            } else {
                // no tree selection means local state table
                self.node_list_ctx(table, current_item);
            }
        }
    }
    unsafe fn node_list_ctx(
        self: &Rc<Self>,
        table: &QPtr<QTableWidget>,
        current_item: Ptr<QTableWidgetItem>,
    ) {
        let row = current_item.row();
        let node = table.item(row, 0).text().to_std_string();
        let pos = table.visual_item_rect(current_item).bottom_left();
        let global_pos = current_item.table_widget().map_to_global(&pos);
        self.ctx_nodes(&node, global_pos);
    }
    unsafe fn svc_list_ctx(
        self: &Rc<Self>,
        table: &QPtr<QTableWidget>,
        current_item: Ptr<QTableWidgetItem>,
        node: &str,
    ) {
        let row = current_item.row();
        let pos = table.visual_item_rect(current_item).bottom_left();
        let global_pos = current_item.table_widget().map_to_global(&pos);
        if let Some(svcs) = table.selected_resources() {
            let current_svc = table.item(row, 0).text().to_std_string();
            self.ctx_svcs(svcs, current_svc, global_pos, node);
        }
    }
    unsafe fn action_list_ctx(
        self: &Rc<Self>,
        table: &QPtr<QTableWidget>,
        current_item: Ptr<QTableWidgetItem>,
        node: &str,
    ) {
        let row = current_item.row();
        let pos = table.visual_item_rect(current_item).bottom_left();
        let global_pos = current_item.table_widget().map_to_global(&pos);
        if let Some(items) = table.selected_resources() {
            let current_action = table.item(row, 1).text().to_std_string();
            self.ctx_actions(items, current_action, global_pos, node);
        }
    }
    unsafe fn item_list_ctx(
        self: &Rc<Self>,
        table: &QPtr<QTableWidget>,
        current_item: Ptr<QTableWidgetItem>,
        node: &str,
    ) {
        let row = current_item.row();
        let pos = table.visual_item_rect(current_item).bottom_left();
        let global_pos = current_item.table_widget().map_to_global(&pos);
        if let Some(items) = table.selected_resources() {
            let current_eva_item = table.item(row, 0).text().to_std_string();
            self.ctx_items(items, current_eva_item, global_pos, node);
        }
    }
    // handle tree popup
    #[slot(SlotNoArgs)]
    unsafe fn on_tree_ctx(self: &Rc<Self>) {
        let current_item = self.window.main_tree.current_item();
        if !current_item.is_null() {
            let widget = current_item.tree_widget();
            if !widget.is_null() {
                if let Some(nd) = self.current_nd() {
                    match nd.kind() {
                        NitKind::State => {
                            let pos = self
                                .window
                                .main_tree
                                .visual_item_rect(current_item)
                                .bottom_left();
                            let global_pos = current_item.tree_widget().map_to_global(&pos);
                            self.ctx_nodes(nd.node(), global_pos);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    unsafe fn confirm(self: &Rc<Self>, text: &str) -> bool {
        let res = QMessageBox::question_q_widget2_q_string(
            &self.window.widget,
            &qs("Confirm"),
            &qs(format!("{}<br><br>Please confirm the operation", text)),
        );
        res.to_int() == 16384
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_copy(self: &Rc<Self>) {
        let result = copy_from_table(&[&self.window.primary_table, &self.window.secondary_table]);
        let mut clipboard = Clipboard::new().unwrap();
        clipboard.set_text(result).unwrap();
    }
    unsafe fn unselect_all(self: &Rc<Self>) {
        for table in [&self.window.primary_table, &self.window.secondary_table] {
            if !table.current_item().is_null() {
                let range = QTableWidgetSelectionRange::new_4a(
                    0,
                    0,
                    table.row_count() - 1,
                    table.column_count() - 1,
                );
                table.set_range_selected(&range, false);
                break;
            }
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_select_all(self: &Rc<Self>) {
        for table in [&self.window.primary_table, &self.window.secondary_table] {
            if !table.current_item().is_null() {
                let range = QTableWidgetSelectionRange::new_4a(
                    0,
                    0,
                    table.row_count() - 1,
                    table.column_count() - 1,
                );
                table.set_range_selected(&range, true);
                break;
            }
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_toggle_auto_reload(self: &Rc<Self>) {
        self.auto_reload_auto_suspended
            .store(false, atomic::Ordering::SeqCst);
        let btn = &self.window.btn_auto_reload_start_stop;
        if btn.text().to_std_string().ends_with('d') {
            self.suspend_auto_reload();
            btn.set_text(&qs("Re&sume"));
            self.window
                .btn_auto_reload_start_stop
                .set_icon(&self.icon_start);
        } else {
            self.unselect_all();
            self.refire_auto_reload();
        }
    }
    #[slot(SlotOfDouble)]
    unsafe fn on_auto_reload_changed(self: &Rc<Self>, value: f64) {
        self.fire_auto_reload(value);
    }
    unsafe fn refire_auto_reload(self: &Rc<Self>) {
        self.fire_auto_reload(self.window.auto_reload.value());
        self.window
            .btn_auto_reload_start_stop
            .set_text(&qs("&Suspend"));
        self.window
            .btn_auto_reload_start_stop
            .set_icon(&self.icon_stop);
    }
    #[slot(SlotNoArgs)]
    unsafe fn s_suspend_auto_reload(self: &Rc<Self>) {
        let btn = &self.window.btn_auto_reload_start_stop;
        if btn.text().to_std_string().ends_with('d') {
            self.on_toggle_auto_reload();
        }
        self.auto_reload_auto_suspended
            .store(true, atomic::Ordering::SeqCst);
    }
    fn suspend_auto_reload(self: &Rc<Self>) {
        self.auto_reload_timer.lock().unwrap().take();
    }
    #[allow(clippy::cast_possible_truncation)]
    unsafe fn fire_auto_reload(self: &Rc<Self>, value: f64) {
        if value <= 0.0 {
            self.auto_reload_timer.lock().unwrap().take();
        } else {
            let timer: QBox<QTimer> = QTimer::new_0a();
            timer.timeout().connect(&self.slot_handle_auto_reload());
            timer.start_1a((value * 1_000.0) as c_int);
            self.auto_reload_timer.lock().unwrap().replace(timer);
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn handle_auto_reload(self: &Rc<Self>) {
        let nit_opt = crate::LAST_NIT.lock().unwrap().clone();
        if let Some(nit) = nit_opt {
            self.process_nit(nit);
        }
    }
    unsafe fn set_item_filter(self: &Rc<Self>, visible: bool) {
        self.window.label_oid.set_visible(visible);
        self.window.i_oid.set_visible(visible);
        self.window.label_node.set_visible(visible);
        self.window.i_node.set_visible(visible);
    }
    unsafe fn set_log_filter(self: &Rc<Self>, visible: bool) {
        self.window.label_log_rx.set_visible(visible);
        self.window.i_log_rx.set_visible(visible);
        self.window.label_log_module.set_visible(visible);
        self.window.i_log_module.set_visible(visible);
        self.window.label_log_limit.set_visible(visible);
        self.window.i_log_limit.set_visible(visible);
        self.window.label_log_time.set_visible(visible);
        self.window.i_log_time.set_visible(visible);
        self.window.label_log_level.set_visible(visible);
        self.window.i_log_level.set_visible(visible);
    }
    unsafe fn set_user_filter(self: &Rc<Self>, visible: bool) {
        self.window.label_user_service.set_visible(visible);
        self.window.i_user_service.set_visible(visible);
    }
    unsafe fn set_action_filter(self: &Rc<Self>, visible: bool) {
        self.window.label_action_oid.set_visible(visible);
        self.window.i_action_oid.set_visible(visible);
        self.window.label_action_service.set_visible(visible);
        self.window.i_action_service.set_visible(visible);
        self.window.label_action_limit.set_visible(visible);
        self.window.i_action_limit.set_visible(visible);
        self.window.label_action_time.set_visible(visible);
        self.window.i_action_time.set_visible(visible);
        self.window.label_action_status.set_visible(visible);
        self.window.i_action_status.set_visible(visible);
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_main_tree_activated(self: &Rc<Self>) {
        if let Some(nd) = self.current_nd() {
            self.set_item_filter(false);
            self.set_log_filter(false);
            self.set_action_filter(false);
            self.set_user_filter(false);
            match nd.kind() {
                NitKind::Items(_, _) => {
                    self.set_item_filter(true);
                }
                NitKind::Log(_) => {
                    self.set_log_filter(true);
                }
                NitKind::Actions(_) => {
                    self.set_action_filter(true);
                }
                NitKind::Users(_) => {
                    self.set_user_filter(true);
                }
                _ => {}
            }
            self.process_nit(Arc::new(nd));
            if self
                .auto_reload_auto_suspended
                .load(atomic::Ordering::SeqCst)
            {
                self.refire_auto_reload();
            }
        }
    }
    unsafe fn current_nd(self: &Rc<Self>) -> Option<NitData> {
        let item = self.window.main_tree.current_item();
        let mut path = Vec::new();
        let mut curr = item;
        while !curr.is_null() {
            path.push(curr.text(0).to_std_string());
            curr = curr.parent();
        }
        path.reverse();
        crate::common::nd_from_path(
            path.iter()
                .map(String::as_str)
                .collect::<Vec<&str>>()
                .as_slice(),
        )
    }
    unsafe fn reload(self: &Rc<Self>) {
        let nit_opt = crate::LAST_NIT.lock().unwrap().clone();
        if let Some(nit) = nit_opt {
            self.process_nit(nit);
        } else {
            self.error("Reload aborted", "no last operation");
        }
    }
    #[allow(clippy::cast_sign_loss)]
    unsafe fn log_filter(self: &Rc<Self>) -> LogFilter {
        let w = &self.window;
        LogFilter {
            level: Some(w.i_log_level.gs()),
            time: Some(w.i_log_time.value() as u32),
            limit: Some(w.i_log_limit.value() as u32),
            module: w.i_log_module.gso(),
            rx: w.i_log_rx.gso(),
        }
    }
    #[allow(clippy::cast_sign_loss)]
    unsafe fn user_filter(self: &Rc<Self>) -> UsersFilter {
        let w = &self.window;
        UsersFilter {
            svc: w.i_user_service.gso(),
        }
    }
    #[allow(clippy::cast_sign_loss)]
    unsafe fn action_filter(self: &Rc<Self>) -> ActionFilter {
        let w = &self.window;
        ActionFilter {
            i: w.i_action_oid.gso(),
            sq: w.i_action_status.gso(),
            svc: w.i_action_service.gso(),
            time: Some(w.i_action_time.value() as u32),
            limit: Some(w.i_action_limit.value() as u32),
        }
    }
    unsafe fn process_nit(self: &Rc<Self>, mut nit: Nit) {
        macro_rules! err {
            ($e: expr) => {
                self.clear_tables();
                self.error("", $e);
            };
        }
        self.window.set_nit_status("");
        match nit.kind() {
            NitKind::Items(oid, node) => {
                let curr_oid = self.window.i_oid.gso();
                let curr_node = self.window.i_node.gso();
                if *oid != curr_oid || *node != curr_node {
                    nit = Arc::new(NitData::new_item_list(nit.node(), curr_oid, curr_node));
                }
            }
            NitKind::Log(filter) => {
                let curr_filter: LogFilter = self.log_filter();
                if filter.as_ref().map_or(true, |f| f != &curr_filter) {
                    nit = Arc::new(NitData::new_log(nit.node(), curr_filter));
                }
            }
            NitKind::Users(filter) => {
                let curr_filter: UsersFilter = self.user_filter();
                if filter.is_none() {
                    let curr_svc = self.window.i_user_service.gso();
                    let mut curr_svc_exists = false;
                    self.window.i_user_service.clear();
                    let nit_svcs = Arc::new(NitData::new_services(nit.node()));
                    if let Ok(svcs) = bus::call::<Vec<SvcData>>(nit_svcs) {
                        for svc in svcs {
                            if !curr_svc_exists
                                && curr_svc.as_ref().map_or(false, |id| id == &svc.id)
                            {
                                curr_svc_exists = true;
                            }
                            if svc.id.starts_with(crate::AAA_SVC_PFX) {
                                self.window.i_user_service.add_item_q_string(&qs(svc.id));
                            }
                        }
                    }
                    if curr_svc_exists {
                        self.window
                            .i_user_service
                            .set_current_text(&qs(curr_svc.unwrap()));
                    }
                }
                if filter.as_ref().map_or(true, |f| f != &curr_filter) {
                    nit = Arc::new(NitData::new_users(nit.node(), curr_filter));
                }
            }
            NitKind::Actions(filter) => {
                let curr_filter: ActionFilter = self.action_filter();
                if filter.is_none() {
                    let curr_svc = self.window.i_action_service.gso();
                    let mut curr_svc_exists = false;
                    self.window.i_action_service.clear();
                    self.window.i_action_service.add_item_q_string(&qs(""));
                    let nit_svcs = Arc::new(NitData::new_services(nit.node()));
                    if let Ok(svcs) = bus::call::<Vec<SvcData>>(nit_svcs) {
                        for svc in svcs {
                            if !curr_svc_exists
                                && curr_svc.as_ref().map_or(false, |id| id == &svc.id)
                            {
                                curr_svc_exists = true;
                            }
                            if svc.id.starts_with(crate::CONTROLLER_SVC_PFX) {
                                self.window.i_action_service.add_item_q_string(&qs(svc.id));
                            }
                        }
                    }
                    if curr_svc_exists {
                        self.window
                            .i_action_service
                            .set_current_text(&qs(curr_svc.unwrap()));
                    }
                }
                if filter.as_ref().map_or(true, |f| f != &curr_filter) {
                    nit = Arc::new(NitData::new_actions(nit.node(), curr_filter));
                }
            }
            _ => {}
        }
        crate::LAST_NIT.lock().unwrap().replace(nit.clone());
        match bus::call::<Value>(nit.clone()) {
            Ok(v) => {
                if let Err(e) = output::result(self, nit, v) {
                    err!(e);
                }
            }
            Err(e) => {
                err!(e);
            }
        }
    }
    unsafe fn process_action_nit(self: &Rc<Self>, nit: Nit) {
        self.ui_action(move || Ok(bus::call::<Value>(nit.clone())?.to_string()));
    }
    pub unsafe fn default_error_box<E: fmt::Display>(self: &Rc<Self>, msg: E) {
        self.error_box(None::<&str>, msg);
    }
    unsafe fn error_box<S: fmt::Display, E: fmt::Display>(
        self: &Rc<Self>,
        what: Option<S>,
        msg: E,
    ) {
        let s = if let Some(s) = what {
            qs(s.to_string())
        } else {
            qs("Failed")
        };
        QMessageBox::warning_q_widget2_q_string(&self.window.widget, &s, &qs(msg.to_string()));
    }
    unsafe fn error<S: fmt::Display, E: fmt::Display>(self: &Rc<Self>, what: S, msg: E) {
        let mut s = what.to_string();
        let d = msg.to_string();
        if s.is_empty() {
            s = d;
        } else if !d.is_empty() {
            write!(s, ": {}", d).unwrap();
        }
        self.window.set_nit_status(&format_err!(s));
        //QMessageBox::warning_q_widget2_q_string(
        //&self.window.widget,
        //&qs("Failed"),
        //&qs(e.message().unwrap_or_default()),
        //);
    }
    #[slot(SlotOfQString)]
    unsafe fn on_proto_selected(self: &Rc<Self>, current: Ref<QString>) {
        self.dialog_connect.handle_proto(&current.to_std_string());
    }
    //#[slot(SlotNoArgs)]
    //unsafe fn on_button_clicked(self: &Rc<Self>) {
    //self.window.status.set_text(&qs("Reloading..."));
    //let code = CString::new("alert('reloading')").unwrap();
    //qwebview_eval(self.window.webview.as_mut_raw_ptr(), code.as_ptr());
    //}
    //#[slot(SlotNoArgs)]
    //unsafe fn on_button_restart(self: &Rc<Self>) {
    //self.clear_workspace();
    //let url = CString::new("http://eva-ics.com/").unwrap();
    //qwebview_load(self.window.webview.as_mut_raw_ptr(), url.as_ptr());
    //}
    #[slot(SlotNoArgs)]
    unsafe fn cleanup(self: &Rc<Self>) {
        if !self.window.widget.is_visible() {
            self.svc_edit_dialogs.close_all();
            self.item_edit_dialogs.close_all();
            self.item_watch_dialogs.close_all();
            self.svc_call_dialogs.close_all();
        }
        self.svc_edit_dialogs.cleanup();
        self.item_edit_dialogs.cleanup();
        self.item_watch_dialogs.cleanup();
        self.svc_call_dialogs.cleanup();
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_action_connect(self: &Rc<Self>) {
        self.dialog_connect.show();
    }
    #[allow(clippy::unused_self)]
    #[slot(SlotNoArgs)]
    unsafe fn on_action_disconnect(self: &Rc<Self>) {
        bus::disconnect();
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_about(self: &Rc<Self>) {
        self.dialog_about.show();
    }
    pub unsafe fn terminate(self: &Rc<Self>) {
        bus::disconnect();
        self.clear_workspace();
        if let Some(config) = self.config.lock().unwrap().as_ref() {
            config.save_to_disk();
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_connect_pressed(self: &Rc<Self>) {
        let opts = self.dialog_connect.generate_options();
        if let Some(config) = self.config.lock().unwrap().as_mut() {
            config.set_connection(&opts);
        }
        bus::connect(opts);
    }
    pub unsafe fn show(self: &Rc<Self>) {
        macro_rules! default_config {
            () => {
                (
                    Config::new(
                        self.dialog_connect.generate_options(),
                        self.window.auto_reload.value(),
                    ),
                    false,
                )
            };
        }
        self.window.widget.show();
        let (config, loaded) = match Config::load_from_disk() {
            Err(e) => {
                eprintln!("unable to load config file: {}", e);
                default_config!()
            }
            Ok(v) => {
                if let Some(c) = v {
                    (c, true)
                } else {
                    default_config!()
                }
            }
        };
        if loaded {
            config.qt_apply(self);
        }
        self.config.lock().unwrap().replace(config);
        if !loaded {
            self.init_splitters();
        }
        if let Some(opts) = self.args.connection_options() {
            self.dialog_connect.set_data(opts.clone());
            if (opts.path.starts_with("http://") || opts.path.starts_with("https://"))
                && opts
                    .credentials
                    .as_ref()
                    .map_or(true, |creds| creds.1.is_empty())
            {
                self.dialog_connect.show();
            } else {
                bus::connect(opts);
            }
        } else {
            self.dialog_connect.show();
        }
    }
}
