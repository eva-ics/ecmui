use crate::bus;
use crate::common::{
    self, copy_from_table, new_size, splitter_sizes, ActionRecordFull, ConnectionOptions,
    ItemActionConfig, ItemConfig, ItemInfo, ItemLogicConfig, ItemState, NitData, PayloadAction,
    PayloadLvarSet, SPointInfo, ServiceParams, SvcData, SvcInfo, SvcMethodInfoParam,
};
use crate::output;
use crate::smart_table::{FormattedValue, FormattedValueColor, Table};
use crate::ui;
use crate::CONTROLLER_SVC_PFX;
use arboard::Clipboard;
use busrt::{DEFAULT_BUF_SIZE, DEFAULT_BUF_TTL, DEFAULT_QUEUE_SIZE};
use chrono::{DateTime, Local, SecondsFormat};
use cpp_core::{Ptr, StaticUpcast};
use eva_common::prelude::*;
use qt_charts::{QChart, QChartView, QLineSeries};
use qt_core::{
    qs, slot, CheckState, QBox, QObject, QPtr, QVariant, SlotNoArgs, SlotOfBool, SlotOfDouble,
    SlotOfQString,
};
use qt_gui::q_key_sequence::StandardKey;
use qt_gui::q_painter::RenderHint;
use qt_gui::{QColor, QKeySequence, QPixmap};
use qt_ui_tools::ui_form;
use qt_widgets::{
    QAction, QCheckBox, QComboBox, QDialogButtonBox, QDoubleSpinBox, QFileDialog, QFormLayout,
    QGridLayout, QLabel, QLineEdit, QPlainTextEdit, QPushButton, QRadioButton, QSpinBox, QSplitter,
    QTabWidget, QTableWidget, QToolButton, QTreeWidget, QWidget,
};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::os::raw::c_int;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::time::Instant;

const OUT_FILE: &str = "Select output file";
pub const IN_FILE: &str = "Select input file";
pub const YAML_FILTER: &str = "*.yml";

thread_local! {
    static LAST_DIR: RefCell<Option<String>> = RefCell::new(None);
}

pub fn get_last_dir() -> String {
    LAST_DIR.with(|cell| {
        if let Some(dir) = cell.borrow().as_ref() {
            (*dir).clone()
        } else {
            ".".to_owned()
        }
    })
}

pub fn set_last_dir(fname: &str) {
    let path = Path::new(fname);
    if let Some(dir) = path.parent() {
        LAST_DIR.with(|cell| cell.borrow_mut().replace(dir.to_string_lossy().to_string()));
    }
}

macro_rules! set_opt_str {
    ($src: expr, $widget: expr, $default: expr) => {
        if let Some(val) = $src {
            $widget.set_text(&qs(val));
        } else {
            $widget.set_text(&qs($default));
        }
    };
}

//macro_rules! set_opt_str_plain {
//($src: expr, $widget: expr, $default: expr) => {
//if let Some(val) = $src {
//$widget.set_plain_text(&qs(val));
//} else {
//$widget.set_plain_text(&qs($default));
//}
//};
//}

pub trait QInputX {
    unsafe fn gs(&self) -> String;
    unsafe fn gso(&self) -> Option<String>;
}

impl QInputX for QLineEdit {
    unsafe fn gs(&self) -> String {
        self.text().to_std_string()
    }
    unsafe fn gso(&self) -> Option<String> {
        let s = self.gs();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

impl QInputX for QPlainTextEdit {
    unsafe fn gs(&self) -> String {
        self.to_plain_text().to_std_string()
    }
    unsafe fn gso(&self) -> Option<String> {
        let s = self.gs();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

impl QInputX for QComboBox {
    unsafe fn gs(&self) -> String {
        self.current_text().to_std_string()
    }
    unsafe fn gso(&self) -> Option<String> {
        let s = self.gs();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

#[ui_form("../ui/main.ui")]
pub struct Main {
    pub(crate) widget: QBox<QWidget>,
    //pub(crate) webview: QPtr<QWidget>,
    //btn_reload: QPtr<QPushButton>,
    pub(crate) btn_auto_reload_start_stop: QPtr<QPushButton>,
    status: QPtr<QLabel>,
    nit_status: QPtr<QLabel>,
    pub(crate) splitter_workspace: QPtr<QSplitter>,
    pub(crate) splitter_tables: QPtr<QSplitter>,
    pub(crate) main_tree: QPtr<QTreeWidget>,
    pub(crate) primary_table: QPtr<QTableWidget>,
    pub(crate) secondary_table: QPtr<QTableWidget>,
    pub(crate) action_connect: QPtr<QAction>,
    pub(crate) action_copy: QPtr<QAction>,
    pub(crate) action_select_all: QPtr<QAction>,
    pub(crate) action_disconnect: QPtr<QAction>,
    pub(crate) action_exit: QPtr<QAction>,
    pub(crate) action_about: QPtr<QAction>,
    pub(crate) action_reload: QPtr<QAction>,
    pub(crate) auto_reload: QPtr<QDoubleSpinBox>,
    pub(crate) action_add_resource: QPtr<QAction>,
    pub(crate) action_edit_resource: QPtr<QAction>,
    pub(crate) action_delete_resource: QPtr<QAction>,
    pub(crate) action_import_resource: QPtr<QAction>,
    pub(crate) action_export_resource: QPtr<QAction>,
    pub(crate) label_oid: QPtr<QLabel>,
    pub(crate) i_oid: QPtr<QLineEdit>,
    pub(crate) label_node: QPtr<QLabel>,
    pub(crate) i_node: QPtr<QComboBox>,
    pub(crate) label_log_rx: QPtr<QLabel>,
    pub(crate) i_log_rx: QPtr<QLineEdit>,
    pub(crate) label_log_module: QPtr<QLabel>,
    pub(crate) i_log_module: QPtr<QLineEdit>,
    pub(crate) label_log_limit: QPtr<QLabel>,
    pub(crate) i_log_limit: QPtr<QSpinBox>,
    pub(crate) label_log_time: QPtr<QLabel>,
    pub(crate) i_log_time: QPtr<QSpinBox>,
    pub(crate) label_log_level: QPtr<QLabel>,
    pub(crate) i_log_level: QPtr<QComboBox>,
    pub(crate) label_action_oid: QPtr<QLabel>,
    pub(crate) i_action_oid: QPtr<QLineEdit>,
    pub(crate) label_action_service: QPtr<QLabel>,
    pub(crate) i_action_service: QPtr<QComboBox>,
    pub(crate) label_user_service: QPtr<QLabel>,
    pub(crate) i_user_service: QPtr<QComboBox>,
    pub(crate) label_action_limit: QPtr<QLabel>,
    pub(crate) i_action_limit: QPtr<QSpinBox>,
    pub(crate) label_action_time: QPtr<QLabel>,
    pub(crate) i_action_time: QPtr<QSpinBox>,
    pub(crate) label_action_status: QPtr<QLabel>,
    pub(crate) i_action_status: QPtr<QComboBox>,
}

impl Main {
    pub unsafe fn clear_workspace(&self) {
        self.main_tree.clear();
        self.clear_primary_table();
        self.clear_secondary_table();
    }
    #[allow(dead_code)]
    pub unsafe fn clear_tables(&self) {
        self.clear_primary_table();
        self.clear_secondary_table();
    }
    pub unsafe fn clear_primary_table(&self) {
        self.primary_table.set_row_count(0);
        self.primary_table.set_column_count(0);
    }
    pub unsafe fn clear_secondary_table(&self) {
        self.secondary_table.set_row_count(0);
        self.secondary_table.set_column_count(0);
    }
    pub unsafe fn set_status(&self, status: &str) {
        self.status.set_text(&qs(status));
    }
    pub unsafe fn set_nit_status(&self, status: &str) {
        self.nit_status.set_text(&qs(status));
    }
}

#[ui_form("../ui/d_connect.ui")]
pub struct DialogConnect {
    widget: QBox<QWidget>,
    pub(crate) proto: QPtr<QComboBox>,
    path: QPtr<QLineEdit>,
    login: QPtr<QLineEdit>,
    password: QPtr<QLineEdit>,
    timeout: QPtr<QSpinBox>,
    pub(crate) button_box: QPtr<QDialogButtonBox>,
}

impl DialogConnect {
    pub unsafe fn show(&self) {
        self.widget.show();
        self.path.set_focus_0a();
    }
}

impl DialogConnect {
    //pub unsafe fn init(&self) {}
    pub unsafe fn handle_proto(&self, proto: &str) {
        if proto == "rt://" {
            self.login.set_disabled(true);
            self.password.set_disabled(true);
        } else {
            self.login.set_disabled(false);
            self.password.set_disabled(false);
        }
    }
    #[allow(clippy::cast_sign_loss)]
    pub unsafe fn generate_options(&self) -> ConnectionOptions {
        let proto = self.proto.gs();
        let path = self.path.gs();
        let timeout = Duration::from_secs(self.timeout.value() as u64);
        if proto == "rt://" {
            ConnectionOptions {
                path,
                credentials: None,
                timeout,
            }
        } else {
            let uri = format!("{}{}", proto, path);
            let login = self.login.text().to_std_string();
            let password = self.password.text().to_std_string();
            ConnectionOptions {
                path: uri,
                credentials: Some((login, password)),
                timeout,
            }
        }
    }
    #[allow(clippy::cast_possible_truncation)]
    pub unsafe fn set_data(&self, opts: ConnectionOptions) {
        macro_rules! set_path {
            ($proto: expr, $path: expr) => {
                self.proto.set_current_text(&qs($proto));
                self.path.set_text(&qs($path));
                self.handle_proto($proto);
            };
        }
        let need_creds = if let Some(path) = opts.path.strip_prefix("http://") {
            set_path!("http://", path);
            true
        } else if let Some(path) = opts.path.strip_prefix("https://") {
            set_path!("https://", path);
            true
        } else {
            set_path!("rt://", opts.path);
            false
        };
        if need_creds {
            if let Some((login, password)) = opts.credentials {
                self.login.set_text(&qs(login));
                self.password.set_text(&qs(password));
            }
        }
        self.timeout.set_value(opts.timeout.as_secs() as c_int);
    }
}

#[ui_form("../ui/d_about.ui")]
pub struct DialogAbout {
    widget: QBox<QWidget>,
    label_version: QPtr<QLabel>,
    btn_ok: QPtr<QPushButton>,
}

impl DialogAbout {
    pub unsafe fn init(self: &Rc<Self>) {
        self.label_version
            .set_text(&qs(format!("version {}", crate::VERSION)));
        let this: Rc<Self> = self.clone();
        self.btn_ok
            .clicked()
            .connect(&SlotNoArgs::new(&self.widget, move || {
                this.widget.close();
            }));
    }
    pub unsafe fn show(&self) {
        self.widget.show();
    }
}

#[ui_form("../ui/busy.ui")]
pub struct Busy {
    widget: QBox<QWidget>,
    status: QPtr<QLabel>,
    gears: QPtr<QLabel>,
    btn_close: QPtr<QPushButton>,
}

impl Busy {
    pub unsafe fn init(self: &Rc<Self>) {
        let this: Rc<Self> = self.clone();
        self.btn_close
            .clicked()
            .connect(&SlotNoArgs::new(&self.widget, move || {
                this.widget.close();
            }));
    }
    pub unsafe fn show(&self) {
        let pixmap = QPixmap::new();
        pixmap.load_1a(&qs(":/i/icons/gears.png"));
        self.gears.set_pixmap(&pixmap);
        self.status.set_text(&qs("Working..."));
        self.widget.set_window_title(&qs("Operation in progress"));
        self.btn_close.hide();
        self.widget.show();
    }
    pub unsafe fn mark_completed(&self, memo: &str) {
        let pixmap = QPixmap::new();
        pixmap.load_1a(&qs(":/i/icons/completed.png"));
        self.gears.set_pixmap(&pixmap);
        self.status.set_text(&qs(memo));
        self.widget.set_window_title(&qs("Completed"));
        self.btn_close.show();
    }
    pub unsafe fn mark_failed(&self, memo: &str) {
        let pixmap = QPixmap::new();
        pixmap.load_1a(&qs(":/i/icons/error.png"));
        self.gears.set_pixmap(&pixmap);
        self.status.set_text(&qs(memo));
        self.widget.set_window_title(&qs("Failed"));
    }
}

pub trait NonModalInfoDialog {
    unsafe fn widget(&self) -> Ptr<QWidget>;
    fn btn_close(&self) -> &QPushButton;
    unsafe fn push(&self, data: EResult<Value>);
    unsafe fn close(&self);
}

pub trait NonModalDialog {
    unsafe fn widget(&self) -> Ptr<QWidget>;
    fn btn_box(&self) -> &QDialogButtonBox;
}

struct InfoDialogInstance<T: NonModalInfoDialog + 'static> {
    dialog: Rc<T>,
    _slot_close: QBox<SlotNoArgs>,
}

pub struct InfoDialogFactory<T: NonModalInfoDialog + 'static> {
    object_registry: Arc<Mutex<HashMap<uuid::Uuid, InfoDialogInstance<T>>>>,
}

impl<T> InfoDialogFactory<T>
where
    T: NonModalInfoDialog + 'static,
{
    pub unsafe fn register(&self, dialog: Rc<T>) -> uuid::Uuid {
        let uuid = uuid::Uuid::new_v4();
        let reg = self.object_registry.clone();
        let dialog_c = dialog.clone();
        let slot_close = SlotNoArgs::new(dialog.widget(), move || {
            reg.lock().unwrap().remove(&uuid);
            dialog_c.close();
        });
        dialog.btn_close().clicked().connect(&slot_close);
        self.object_registry.lock().unwrap().insert(
            uuid,
            InfoDialogInstance {
                dialog,
                _slot_close: slot_close,
            },
        );
        uuid
    }
    pub unsafe fn push(&self, u: uuid::Uuid, data: EResult<Value>) -> bool {
        let mut reg = self.object_registry.lock().unwrap();
        if let Some(i) = reg.get(&u) {
            if i.dialog.widget().is_visible() {
                i.dialog.push(data);
                true
            } else {
                reg.remove(&u);
                false
            }
        } else {
            false
        }
    }
    pub unsafe fn close_all(&self) {
        let mut objs = self.object_registry.lock().unwrap();
        for obj in objs.values() {
            obj.dialog.close();
        }
        objs.clear();
    }
    pub unsafe fn cleanup(&self) {
        self.object_registry
            .lock()
            .unwrap()
            .retain(|_, v| v.dialog.widget().is_visible());
    }
}

impl<T> Default for InfoDialogFactory<T>
where
    T: NonModalInfoDialog + 'static,
{
    fn default() -> Self {
        Self {
            object_registry: <_>::default(),
        }
    }
}

#[derive(bmart_derive::EnumStr)]
#[enumstr(rename_all = "CamelCase")]
pub enum DialogKind {
    Add,
    Edit,
}

struct DialogInstance<T: NonModalDialog + 'static> {
    dialog: Rc<T>,
    node: String,
}

pub struct DialogFactory<T: NonModalDialog + 'static> {
    object_registry: Arc<Mutex<HashMap<uuid::Uuid, DialogInstance<T>>>>,
}

impl<T> Default for DialogFactory<T>
where
    T: NonModalDialog + 'static,
{
    fn default() -> Self {
        Self {
            object_registry: <_>::default(),
        }
    }
}

impl<T> DialogFactory<T>
where
    T: NonModalDialog + 'static,
{
    pub unsafe fn register<F>(&self, dialog: Rc<T>, node: &str, mut process: F)
    where
        F: FnMut(Rc<T>, &str) -> bool + 'static,
    {
        let uuid = uuid::Uuid::new_v4();
        let reg = self.object_registry.clone();
        let slot_accepted = SlotNoArgs::new(dialog.widget(), move || {
            let instance_o = reg.lock().unwrap().remove(&uuid);
            if let Some(instance) = instance_o {
                if !process(instance.dialog.clone(), &instance.node) {
                    instance.dialog.widget().show();
                    reg.lock().unwrap().insert(uuid, instance);
                }
            }
        });
        let reg = self.object_registry.clone();
        let slot_rejected = SlotNoArgs::new(dialog.widget(), move || {
            reg.lock().unwrap().remove(&uuid);
        });
        dialog.btn_box().accepted().connect(&slot_accepted);
        dialog.btn_box().rejected().connect(&slot_rejected);
        self.object_registry.lock().unwrap().insert(
            uuid,
            DialogInstance {
                dialog,
                node: node.to_owned(),
            },
        );
    }
    pub unsafe fn close_all(&self) {
        let mut objs = self.object_registry.lock().unwrap();
        for obj in objs.values() {
            obj.dialog.widget().close();
        }
        objs.clear();
    }
    pub unsafe fn cleanup(&self) {
        self.object_registry
            .lock()
            .unwrap()
            .retain(|_, v| v.dialog.widget().is_visible());
    }
}

pub struct ExportConfig {
    pub file: Option<String>,
    pub kind: ExportKind,
    pub cloud_node: Option<String>,
    pub merge: bool,
}

#[derive(Eq, PartialEq)]
pub enum ExportKind {
    Resource,
    CloudDeploy,
}

#[ui_form("../ui/d_export.ui")]
pub struct DialogExport {
    pub(crate) widget: QBox<QWidget>,
    i_file: QPtr<QLineEdit>,
    rb_cloud: QPtr<QRadioButton>,
    rb_res: QPtr<QRadioButton>,
    i_node: QPtr<QLineEdit>,
    cb_merge: QPtr<QCheckBox>,
    btn_select_file: QPtr<QToolButton>,
    btnbox: QPtr<QDialogButtonBox>,
}

impl DialogExport {
    pub unsafe fn export_config(self: &Rc<Self>) -> ExportConfig {
        ExportConfig {
            file: self.i_file.gso(),
            kind: if self.rb_res.is_checked() {
                ExportKind::Resource
            } else {
                ExportKind::CloudDeploy
            },
            cloud_node: self.i_node.gso(),
            merge: self.cb_merge.is_checked(),
        }
    }
    pub unsafe fn init(self: &Rc<Self>, on_submit: &QBox<SlotNoArgs>) {
        let this: Rc<Self> = self.clone();
        self.rb_res
            .clicked()
            .connect(&SlotOfBool::new(&self.widget, move |checked| {
                this.i_node.set_enabled(!checked);
            }));
        let this: Rc<Self> = self.clone();
        self.rb_cloud
            .clicked()
            .connect(&SlotOfBool::new(&self.widget, move |checked| {
                this.i_node.set_enabled(checked);
            }));
        let this: Rc<Self> = self.clone();
        self.btn_select_file
            .clicked()
            .connect(&SlotNoArgs::new(&self.widget, move || {
                let fname = /* if this.cb_merge.is_checked() {
                    QFileDialog::get_open_file_name_4a(
                        &this.widget,
                        &qs(OUT_FILE),
                        &qs(get_last_dir()),
                        &qs(YAML_FILTER),
                    )
                } else {*/
                    QFileDialog::get_save_file_name_4a(
                        &this.widget,
                        &qs(OUT_FILE),
                        &qs(get_last_dir()),
                        &qs(YAML_FILTER),
                    )
                //}
                .to_std_string();
                if !fname.is_empty() {
                    set_last_dir(&fname);
                    this.i_file.set_text(&qs(fname));
                }
            }));
        self.btnbox.accepted().connect(on_submit);
    }
    pub unsafe fn show0(&self) {
        self.widget.show();
    }
    pub unsafe fn show(&self, node: &str, res: &str) {
        self.widget.set_window_title(&qs(format!("Export {res}")));
        self.i_node.set_text(&qs(node));
        self.widget.show();
    }
}

#[ui_form("../ui/svc_edit.ui")]
pub struct DialogSvcEdit {
    pub(crate) widget: QBox<QWidget>,
    i_id: QPtr<QLineEdit>,
    i_command: QPtr<QLineEdit>,
    i_prepare_command: QPtr<QLineEdit>,
    i_config: QPtr<QPlainTextEdit>,
    i_launcher: QPtr<QComboBox>,
    i_enabled: QPtr<QCheckBox>,
    i_react_to_fail: QPtr<QCheckBox>,
    i_call_tracing: QPtr<QCheckBox>,
    i_user: QPtr<QLineEdit>,
    i_workers: QPtr<QSpinBox>,
    i_timeout_default: QPtr<QDoubleSpinBox>,
    i_timeout_startup: QPtr<QDoubleSpinBox>,
    i_timeout_shutdown: QPtr<QDoubleSpinBox>,
    i_bus_type: QPtr<QComboBox>,
    i_bus_path: QPtr<QLineEdit>,
    i_bus_timeout: QPtr<QDoubleSpinBox>,
    i_bus_buf_size: QPtr<QSpinBox>,
    i_bus_buf_ttl: QPtr<QSpinBox>,
    i_bus_queue_size: QPtr<QSpinBox>,
    i_bus_ping_interval: QPtr<QDoubleSpinBox>,
    label_load_tpl: QPtr<QLabel>,
    pub(crate) btn_load_tpl: QPtr<QToolButton>,
    pub(crate) btnbox: QPtr<QDialogButtonBox>,
}

impl NonModalDialog for DialogSvcEdit {
    unsafe fn widget(&self) -> Ptr<QWidget> {
        self.widget.as_ptr()
    }
    fn btn_box(&self) -> &QDialogButtonBox {
        &self.btnbox
    }
}

pub unsafe fn on_svc_btn_load_clicked(dialog: &DialogSvcEdit, node: &str, ui_obj: &Rc<ui::Ui>) {
    let path = Path::new("/opt/eva4/share/svc-tpl");
    let mut need_set = false;
    let dir = if path.exists() {
        qs(path.to_string_lossy())
    } else {
        need_set = true;
        qs(get_last_dir())
    };
    let fname = QFileDialog::get_open_file_name_4a(
        &dialog.widget,
        &qs("Select service template"),
        &dir,
        &qs(YAML_FILTER),
    )
    .to_std_string();
    if !fname.is_empty() {
        if need_set {
            set_last_dir(&fname);
        }
        match ServiceParams::load_from_disk(&fname) {
            Ok(params) => {
                let nit = Arc::new(NitData::new_spoints(node));
                if let Ok(spoints) = bus::call::<Vec<SPointInfo>>(nit) {
                    dialog.apply_params(params, spoints);
                } else {
                    ui_obj.default_error_box(ui::ERR_LOAD_SPOINTS);
                }
            }
            Err(e) => ui_obj.default_error_box(e),
        }
    }
}

impl DialogSvcEdit {
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_possible_wrap)]
    pub unsafe fn show_add(&self, node: &str, spoints: Vec<SPointInfo>) {
        self.label_load_tpl.show();
        self.btn_load_tpl.show();
        self.i_id.set_text(&qs("eva."));
        self.i_id.set_enabled(true);
        self.i_command.set_text(&qs(""));
        self.i_prepare_command.set_text(&qs(""));
        self.i_config.set_plain_text(&qs(""));
        self.i_enabled.set_check_state(CheckState::Checked);
        self.i_react_to_fail.set_check_state(CheckState::Unchecked);
        self.i_call_tracing.set_check_state(CheckState::Unchecked);
        self.i_user.set_text(&qs(""));
        self.i_workers.set_value(1);
        self.i_launcher.clear();
        self.i_launcher
            .add_item_q_string(&qs(common::LAUNCHER_MAIN));
        for spoint in spoints {
            if let Some(name) = spoint.short_name() {
                self.i_launcher
                    .add_item_q_string(&qs(format!("{}{}", common::LAUNCHER_PFX, name)));
            }
        }
        self.i_timeout_default
            .set_value(common::DEFAULT_TIMEOUT_SEC);
        self.i_timeout_startup
            .set_value(common::DEFAULT_TIMEOUT_SEC);
        self.i_timeout_shutdown
            .set_value(common::DEFAULT_TIMEOUT_SEC);
        self.i_bus_type.clear();
        self.i_bus_type
            .add_item_q_string(&qs(common::DEFAULT_BUS_TYPE));
        self.i_bus_type
            .set_current_text(&qs(common::DEFAULT_BUS_TYPE));
        self.i_bus_path.set_text(&qs(common::DEFAULT_BUS_PATH));
        self.i_bus_timeout.set_value(common::DEFAULT_TIMEOUT_SEC);
        self.i_bus_buf_size.set_value(DEFAULT_BUF_SIZE as i32);
        self.i_bus_buf_ttl
            .set_value(DEFAULT_BUF_TTL.as_micros() as i32);
        self.i_bus_queue_size.set_value(DEFAULT_QUEUE_SIZE as i32);
        self.i_bus_ping_interval
            .set_value(common::DEFAULT_BUS_PING_INTERVAL_SEC);
        self.widget
            .set_window_title(&qs(format!("Add service to {}", node)));
        self.widget.show();
    }
    pub unsafe fn apply_params(&self, params: ServiceParams, spoints: Vec<SPointInfo>) {
        if let Some(id) = params.id {
            self.i_id.set_text(&qs(id));
        }
        self.i_command.set_text(&qs(params.command));
        set_opt_str!(params.prepare_command, self.i_prepare_command, "");
        match serde_yaml::to_string(&params.config) {
            Ok(v) => self.i_config.set_plain_text(&qs(v)),
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        }
        self.i_enabled.set_check_state(if params.enabled {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        });
        self.i_react_to_fail
            .set_check_state(if params.react_to_fail {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            });
        self.i_call_tracing.set_check_state(if params.call_tracing {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        });
        set_opt_str!(params.user, self.i_user, "");
        self.i_workers.set_value(params.workers);
        self.i_launcher.clear();
        self.i_launcher
            .add_item_q_string(&qs(common::LAUNCHER_MAIN));
        for spoint in spoints {
            if let Some(name) = spoint.short_name() {
                self.i_launcher
                    .add_item_q_string(&qs(format!("{}{}", common::LAUNCHER_PFX, name)));
            }
        }
        self.i_launcher.set_current_text(&qs(params.launcher));
        let default_timeout = params
            .timeout
            .default
            .unwrap_or(common::DEFAULT_TIMEOUT_SEC);
        self.i_timeout_default.set_value(default_timeout);
        self.i_timeout_startup
            .set_value(params.timeout.startup.unwrap_or(default_timeout));
        self.i_timeout_shutdown
            .set_value(params.timeout.shutdown.unwrap_or(default_timeout));
        self.i_bus_type.clear();
        self.i_bus_type
            .add_item_q_string(&qs(common::DEFAULT_BUS_TYPE));
        self.i_bus_type.set_current_text(&qs(params.bus.kind));
        self.i_bus_path.set_text(&qs(params.bus.path));
        self.i_bus_timeout
            .set_value(params.bus.timeout.unwrap_or(common::DEFAULT_TIMEOUT_SEC));
        self.i_bus_buf_size.set_value(params.bus.buf_size);
        self.i_bus_buf_ttl.set_value(params.bus.buf_ttl);
        self.i_bus_queue_size.set_value(params.bus.queue_size);
        self.i_bus_ping_interval.set_value(params.bus.ping_interval);
    }
    pub unsafe fn show_edit(&self, node: &str, params: ServiceParams, spoints: Vec<SPointInfo>) {
        if let Some(ref id) = params.id {
            self.label_load_tpl.hide();
            self.btn_load_tpl.hide();
            self.widget
                .set_window_title(&qs(format!("Edit service {}/{}", node, id)));
            self.i_id.set_enabled(false);
            self.apply_params(params, spoints);
            self.widget.show();
        }
    }
    #[allow(clippy::float_cmp)]
    pub unsafe fn parse_params(&self) -> EResult<ServiceParams> {
        let config_str = self.i_config.to_plain_text().to_std_string();
        let config: Value = if config_str.is_empty() {
            Value::Map(BTreeMap::new())
        } else {
            serde_yaml::from_str(&config_str).map_err(Error::invalid_data)?
        };
        let default_timeout = self.i_timeout_default.value();
        let startup_timeout = self.i_timeout_startup.value();
        let shutdown_timeout = self.i_timeout_startup.value();
        let bus_timeout = self.i_bus_timeout.value();
        let timeout = common::TimeoutConfig {
            default: Some(default_timeout),
            startup: if startup_timeout == default_timeout {
                None
            } else {
                Some(startup_timeout)
            },
            shutdown: if shutdown_timeout == default_timeout {
                None
            } else {
                Some(shutdown_timeout)
            },
        };
        let bus = common::BusConfig {
            buf_size: self.i_bus_buf_size.value(),
            buf_ttl: self.i_bus_buf_ttl.value(),
            path: self.i_bus_path.gs(),
            ping_interval: self.i_bus_ping_interval.value(),
            queue_size: self.i_bus_queue_size.value(),
            timeout: if bus_timeout == default_timeout {
                None
            } else {
                Some(bus_timeout)
            },
            kind: self.i_bus_type.gs(),
        };
        Ok(ServiceParams {
            id: self.i_id.gso(),
            command: self.i_command.gs(),
            prepare_command: self.i_prepare_command.gso(),
            enabled: self.i_enabled.check_state() == CheckState::Checked,
            react_to_fail: self.i_react_to_fail.check_state() == CheckState::Checked,
            call_tracing: self.i_call_tracing.check_state() == CheckState::Checked,
            user: self.i_user.gso(),
            config,
            workers: self.i_workers.value(),
            launcher: self.i_launcher.gs(),
            timeout,
            bus,
        })
    }
}

#[ui_form("../ui/item_edit.ui")]
pub struct DialogItemEdit {
    pub(crate) widget: QBox<QWidget>,
    i_kind: QPtr<QComboBox>,
    i_full_id: QPtr<QLineEdit>,
    i_range_min: QPtr<QLineEdit>,
    i_range_min_eq: QPtr<QComboBox>,
    i_range_max: QPtr<QLineEdit>,
    i_range_max_eq: QPtr<QComboBox>,
    i_action_svc: QPtr<QComboBox>,
    i_action_timeout: QPtr<QDoubleSpinBox>,
    i_enabled: QPtr<QCheckBox>,
    i_action_config: QPtr<QPlainTextEdit>,
    i_meta: QPtr<QPlainTextEdit>,
    pub(crate) btnbox: QPtr<QDialogButtonBox>,
}

impl NonModalDialog for DialogItemEdit {
    unsafe fn widget(&self) -> Ptr<QWidget> {
        self.widget.as_ptr()
    }
    fn btn_box(&self) -> &QDialogButtonBox {
        &self.btnbox
    }
}

impl DialogItemEdit {
    pub unsafe fn init(self: &Rc<Self>) {
        let this = self.clone();
        self.i_kind
            .activated2()
            .connect(&SlotOfQString::new(&self.widget, move |val| {
                this.handle_oid_kind(&val.to_std_string());
            }));
    }
    unsafe fn handle_oid_kind(self: &Rc<Self>, kind: &str) {
        // set state-related fields fields
        let enabled = kind != "lmacro:";
        self.i_range_min.set_enabled(enabled);
        self.i_range_max.set_enabled(enabled);
        self.i_range_min_eq.set_enabled(enabled);
        self.i_range_max_eq.set_enabled(enabled);
        // set action-related fields
        let enabled = kind == "lmacro:" || kind == "unit:";
        self.i_action_svc.set_enabled(enabled);
        self.i_action_timeout.set_enabled(enabled);
        self.i_action_config.set_enabled(enabled);
    }
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_possible_wrap)]
    pub unsafe fn show_add(self: &Rc<Self>, node: &str, services: Vec<SvcData>) {
        let kind = "sensor:";
        self.handle_oid_kind(kind);
        self.i_kind.set_current_text(&qs(kind));
        self.i_full_id.set_text(&qs(""));
        self.i_kind.set_enabled(true);
        self.i_full_id.set_enabled(true);
        self.i_range_min.set_text(&qs(""));
        self.i_range_max.set_text(&qs(""));
        self.i_range_min_eq.set_current_text(&qs("≤"));
        self.i_range_max_eq.set_current_text(&qs("≤"));
        self.i_action_svc.clear();
        self.i_action_svc.add_item_q_string(&qs(""));
        for svc in services {
            if svc.id.starts_with(CONTROLLER_SVC_PFX) {
                self.i_action_svc.add_item_q_string(&qs(svc.id));
            }
        }
        self.i_action_svc.set_current_text(&qs(""));
        self.i_action_timeout.set_value(0.0);
        self.i_meta.set_plain_text(&qs(""));
        self.i_enabled.set_checked(true);
        self.widget
            .set_window_title(&qs(format!("Add item to {}", node)));
        self.widget.show();
    }
    pub unsafe fn show_edit(
        self: &Rc<Self>,
        node: &str,
        config: ItemConfig,
        services: Vec<SvcData>,
    ) {
        let kind = format!("{}:", config.oid.kind());
        self.handle_oid_kind(&kind);
        self.i_kind.set_current_text(&qs(kind));
        self.i_full_id.set_text(&qs(config.oid.full_id()));
        self.i_kind.set_enabled(false);
        self.i_full_id.set_enabled(false);
        self.i_range_min.set_text(&qs(""));
        self.i_range_max.set_text(&qs(""));
        self.i_range_min_eq.set_current_text(&qs("≤"));
        self.i_range_max_eq.set_current_text(&qs("≤"));
        if let Some(logic) = config.logic {
            if let Some(range) = logic.range {
                if let Some(min) = range.min {
                    self.i_range_min.set_text(&qs(min.to_string()));
                }
                if let Some(max) = range.max {
                    self.i_range_max.set_text(&qs(max.to_string()));
                }
                if range.min_eq {
                    self.i_range_min_eq.set_current_text(&qs("≤"));
                } else {
                    self.i_range_min_eq.set_current_text(&qs("<"));
                }
                if range.max_eq {
                    self.i_range_max_eq.set_current_text(&qs("≤"));
                } else {
                    self.i_range_max_eq.set_current_text(&qs("<"));
                }
            }
        }
        self.i_action_svc.clear();
        self.i_action_svc.add_item_q_string(&qs(""));
        for svc in services {
            if svc.id.starts_with(CONTROLLER_SVC_PFX) {
                self.i_action_svc.add_item_q_string(&qs(svc.id));
            }
        }
        self.i_action_timeout.set_value(0.0);
        if let Some(action) = config.action {
            if let Some(timeout) = action.timeout {
                self.i_action_timeout.set_value(timeout);
            }
            if let Some(svc) = action.svc {
                self.i_action_svc.set_current_text(&qs(svc));
            }
            if let Some(action_config) = action.config {
                match serde_yaml::to_string(&action_config) {
                    Ok(v) => self.i_action_config.set_plain_text(&qs(v)),
                    Err(e) => {
                        eprintln!("{}", e);
                        return;
                    }
                }
            }
        }
        if let Some(meta) = config.meta {
            match serde_yaml::to_string(&meta) {
                Ok(v) => self.i_meta.set_plain_text(&qs(v)),
                Err(e) => {
                    eprintln!("{}", e);
                    return;
                }
            }
        } else {
            self.i_meta.set_plain_text(&qs(""));
        }
        self.i_enabled.set_checked(config.enabled);
        self.widget
            .set_window_title(&qs(format!("Edit item {}/{}", node, config.oid)));
        self.widget.show();
    }
    //#[allow(clippy::float_cmp)]
    pub unsafe fn parse_config(&self) -> EResult<ItemConfig> {
        let oid: OID = format!("{}{}", self.i_kind.gs(), self.i_full_id.gs()).parse()?;
        let meta_str = self.i_meta.to_plain_text().to_std_string();
        let meta: Option<Value> = if meta_str.is_empty() {
            None
        } else {
            Some(serde_yaml::from_str(&meta_str).map_err(Error::invalid_data)?)
        };
        let action = if let Some(svc) = self.i_action_svc.gso() {
            let timeout = {
                let val = self.i_action_timeout.value();
                if val > 0.0 {
                    Some(val)
                } else {
                    None
                }
            };
            let config_str = self.i_action_config.to_plain_text().to_std_string();
            let config: Option<Value> = if config_str.is_empty() {
                None
            } else {
                Some(serde_yaml::from_str(&config_str).map_err(Error::invalid_data)?)
            };
            Some(ItemActionConfig {
                svc: Some(svc),
                timeout,
                config,
            })
        } else {
            None
        };
        let i_range_min = if let Some(val) = self.i_range_min.gso() {
            Some(val.parse::<f64>()?)
        } else {
            None
        };
        let i_range_max = if let Some(val) = self.i_range_max.gso() {
            Some(val.parse::<f64>()?)
        } else {
            None
        };
        let logic = if i_range_min.is_some() || i_range_max.is_some() {
            let range = eva_common::logic::Range {
                min: i_range_min,
                max: i_range_max,
                min_eq: self.i_range_min_eq.current_text().to_std_string() != "<",
                max_eq: self.i_range_min_eq.current_text().to_std_string() != "<",
            };
            Some(ItemLogicConfig { range: Some(range) })
        } else {
            None
        };
        Ok(ItemConfig {
            oid,
            enabled: self.i_enabled.is_checked(),
            meta,
            action,
            logic,
        })
    }
}

#[ui_form("../ui/lvar_set.ui")]
pub struct DialogLvarSet {
    pub(crate) widget: QBox<QWidget>,
    c_status: QPtr<QCheckBox>,
    c_value: QPtr<QCheckBox>,
    i_status: QPtr<QSpinBox>,
    i_value: QPtr<QPlainTextEdit>,
    pub(crate) btn_box: QPtr<QDialogButtonBox>,
}

impl DialogLvarSet {
    pub unsafe fn init(self: &Rc<Self>) {
        let this = self.clone();
        self.c_status
            .clicked()
            .connect(&SlotOfBool::new(&self.widget, move |checked| {
                this.i_status.set_enabled(checked);
            }));
        let this = self.clone();
        self.c_value
            .clicked()
            .connect(&SlotOfBool::new(&self.widget, move |checked| {
                this.i_value.set_enabled(checked);
            }));
    }
    pub unsafe fn show(self: &Rc<Self>, state: Option<ItemState>) {
        self.c_status.set_checked(false);
        self.c_value.set_checked(false);
        if let Some(st) = state {
            self.i_status.set_value(i32::from(st.status));
            self.i_value
                .set_plain_text(&qs(output::format_value(st.value).to_string()));
        } else {
            self.i_status.set_value(1);
            self.i_value.set_plain_text(&qs(""));
        }
        self.i_status.set_enabled(false);
        self.i_value.set_enabled(false);
        self.widget.show();
    }
    pub unsafe fn parse_payload(self: &Rc<Self>) -> EResult<PayloadLvarSet> {
        let status = if self.c_status.is_checked() {
            Some(
                self.i_status
                    .value()
                    .try_into()
                    .map_err(Error::invalid_data)?,
            )
        } else {
            None
        };
        let value = if self.c_value.is_checked() {
            if let Some(val) = self.i_value.gso() {
                Some(val.parse()?)
            } else {
                None
            }
        } else {
            None
        };
        Ok(PayloadLvarSet {
            i: None,
            status,
            value,
        })
    }
}

struct QKwArg {
    i_name: QBox<QLineEdit>,
    i_value: QBox<QLineEdit>,
}

impl QKwArg {
    unsafe fn new() -> Self {
        Self {
            i_name: QLineEdit::new(),
            i_value: QLineEdit::new(),
        }
    }
}

pub struct DialogLmacroRun {
    pub(crate) qdialog: QDialogLmacroRun,
    inputs_args: Mutex<Vec<Rc<QBox<QLineEdit>>>>,
    inputs_kwargs: Mutex<Vec<Rc<QKwArg>>>,
}

impl DialogLmacroRun {
    pub fn new() -> Self {
        let dialog = QDialogLmacroRun::load();
        Self {
            qdialog: dialog,
            inputs_args: <_>::default(),
            inputs_kwargs: <_>::default(),
        }
    }
    pub unsafe fn parse_payload(self: &Rc<Self>) -> EResult<PayloadAction> {
        let mut args: Vec<Value> = Vec::new();
        let mut kwargs: HashMap<String, Value> = HashMap::new();
        let i_args = self.inputs_args.lock().unwrap();
        let args_opt = if i_args.is_empty() {
            None
        } else {
            let mut skipped = false;
            for a in i_args.iter().rev() {
                if let Some(d) = a.gso() {
                    skipped = true;
                    args.push(d.parse()?);
                } else if skipped {
                    args.push(Value::Unit);
                }
            }
            args.reverse();
            Some(args)
        };
        let i_kwargs = self.inputs_kwargs.lock().unwrap();
        for k in i_kwargs.iter() {
            if let Some(name) = k.i_name.gso() {
                let n = name.trim();
                if !n.is_empty() {
                    if let Some(v) = k.i_value.gso() {
                        kwargs.insert(n.to_owned(), v.parse()?);
                    } else {
                        kwargs.insert(n.to_owned(), Value::Unit);
                    }
                }
            }
        }
        let params = eva_common::actions::LmacroParams {
            args: args_opt,
            kwargs: if kwargs.is_empty() {
                None
            } else {
                Some(kwargs)
            },
        };
        Ok(PayloadAction {
            i: None,
            params: eva_common::actions::Params::Lmacro(params),
        })
    }
    unsafe fn append_input_args(self: &Rc<Self>, n: usize) -> Option<Rc<QBox<QLineEdit>>> {
        let mut input_args = self.inputs_args.lock().unwrap();
        if input_args.len() == n {
            let i_args = Rc::new(QLineEdit::new());
            let this = self.clone();
            let me = i_args.clone();
            i_args
                .text_edited()
                .connect(&SlotNoArgs::new(&self.qdialog.widget, move || {
                    if let Some(i) = this.append_input_args(n + 1) {
                        QWidget::set_tab_order(me.as_ptr(), i.as_ptr());
                    }
                }));
            self.qdialog.form_args.add_row_q_widget(i_args.as_ptr());
            if n == 0 {
                i_args.set_focus_0a();
            }
            input_args.push(i_args.clone());
            Some(i_args)
        } else {
            None
        }
    }
    unsafe fn append_input_kwargs(self: &Rc<Self>, n: usize) -> Option<Rc<QKwArg>> {
        let mut input_kwargs = self.inputs_kwargs.lock().unwrap();
        if input_kwargs.len() == n {
            let qkw = Rc::new(QKwArg::new());
            let this = self.clone();
            let me = qkw.clone();
            qkw.i_name
                .text_edited()
                .connect(&SlotNoArgs::new(&self.qdialog.widget, move || {
                    if let Some(i) = this.append_input_kwargs(n + 1) {
                        QWidget::set_tab_order(me.i_value.as_ptr(), i.i_name.as_ptr());
                        QWidget::set_tab_order(i.i_name.as_ptr(), i.i_value.as_ptr());
                    }
                }));
            self.qdialog
                .form_kwargs
                .add_row_2_q_widget(&qkw.i_name, &qkw.i_value);
            input_kwargs.push(qkw.clone());
            Some(qkw)
        } else {
            None
        }
    }
    pub unsafe fn show(self: &Rc<Self>) {
        self.inputs_args.lock().unwrap().clear();
        self.append_input_args(0);
        self.append_input_kwargs(0);
        self.qdialog.widget.show();
    }
}

#[ui_form("../ui/lmacro_run.ui")]
pub struct QDialogLmacroRun {
    pub(crate) widget: QBox<QWidget>,
    form_args: QPtr<QFormLayout>,
    form_kwargs: QPtr<QFormLayout>,
    pub(crate) btn_box: QPtr<QDialogButtonBox>,
}

#[ui_form("../ui/unit_action.ui")]
pub struct DialogUnitAction {
    pub(crate) widget: QBox<QWidget>,
    i_value: QPtr<QPlainTextEdit>,
    pub(crate) btn_box: QPtr<QDialogButtonBox>,
}

impl DialogUnitAction {
    pub unsafe fn show(self: &Rc<Self>, state: Option<ItemState>) {
        if let Some(st) = state {
            self.i_value
                .set_plain_text(&qs(output::format_value_pretty(st.value).to_string()));
        } else {
            self.i_value.set_plain_text(&qs(""));
        }
        self.widget.show();
    }
    pub unsafe fn parse_payload(self: &Rc<Self>) -> EResult<PayloadAction> {
        let value = self.i_value.gs().parse()?;
        let params = eva_common::actions::UnitParams { value };
        Ok(PayloadAction {
            i: None,
            params: eva_common::actions::Params::Unit(params),
        })
    }
}

pub struct DialogActionWatch {
    qdialog: QDialogActionWatch,
    u: Mutex<Option<uuid::Uuid>>,
}

#[ui_form("../ui/action_watch.ui")]
struct QDialogActionWatch {
    pub(crate) widget: QBox<QWidget>,
    #[allow(dead_code)]
    i_uuid: QPtr<QLineEdit>,
    i_oid: QPtr<QLineEdit>,
    i_node: QPtr<QLineEdit>,
    i_svc: QPtr<QLineEdit>,
    label_status: QPtr<QLabel>,
    i_exitcode: QPtr<QLineEdit>,
    i_params: QPtr<QPlainTextEdit>,
    i_out: QPtr<QPlainTextEdit>,
    i_err: QPtr<QPlainTextEdit>,
    btn_close: QPtr<QPushButton>,
    i_time_elapsed: QPtr<QLineEdit>,
    i_time_created: QPtr<QLineEdit>,
    i_time_accepted: QPtr<QLineEdit>,
    i_time_pending: QPtr<QLineEdit>,
    i_time_running: QPtr<QLineEdit>,
    label_time_result: QPtr<QLabel>,
    i_time_result: QPtr<QLineEdit>,
}

impl DialogActionWatch {
    pub unsafe fn new(node: &str, u: uuid::Uuid) -> Self {
        let dialog = QDialogActionWatch::load();
        dialog
            .widget
            .set_window_title(&qs(format!("{}/{}", node, u)));
        dialog.i_uuid.set_text(&qs(u.to_string()));
        dialog.i_oid.set_text(&qs(""));
        dialog.i_node.set_text(&qs(""));
        dialog.i_svc.set_text(&qs(""));
        dialog.label_status.set_text(&qs(""));
        dialog.i_exitcode.set_text(&qs(""));
        dialog.i_params.set_plain_text(&qs(""));
        dialog.i_out.set_plain_text(&qs(""));
        dialog.i_err.set_plain_text(&qs(""));
        Self {
            qdialog: dialog,
            u: <_>::default(),
        }
    }
    pub unsafe fn init(self: &Rc<Self>, u: uuid::Uuid) {
        self.u.lock().unwrap().replace(u);
    }
    pub unsafe fn show(self: &Rc<Self>) {
        self.qdialog.widget.show();
    }
    unsafe fn stop_watcher(&self) {
        if let Some(u) = self.u.lock().unwrap().as_ref() {
            let _r = bus::call::<()>(Arc::new(NitData::stop_watcher(*u)));
        }
    }
    unsafe fn process_data(&self, data: Value) -> EResult<()> {
        let qdialog = &self.qdialog;
        if data == Value::Unit {
            qdialog.label_status.set_text(&qs("delete from the node"));
            self.stop_watcher();
            return Ok(());
        }
        let a = ActionRecordFull::deserialize(data)?;
        let elapsed = a.elapsed();
        qdialog.i_oid.set_text(&qs(a.oid));
        qdialog.i_node.set_text(&qs(a.node));
        qdialog.i_svc.set_text(&qs(a.svc));
        let color = FormattedValueColor::from_action_status(&a.status);
        qdialog
            .label_status
            .set_text(&qs(color.rich(&a.status, Some("font-weight: bold"))));
        set_opt_str!(a.exitcode.map(|v| v.to_string()), qdialog.i_exitcode, "");
        qdialog
            .i_params
            .set_plain_text(&qs(output::format_value_pretty(a.params).to_string()));
        qdialog
            .i_out
            .set_plain_text(&qs(output::format_value_pretty(a.out).to_string()));
        qdialog
            .i_err
            .set_plain_text(&qs(output::format_value_pretty(a.err).to_string()));
        set_opt_str!(
            a.time.get("created").map(|v| output::time_full(*v).0),
            qdialog.i_time_created,
            ""
        );
        set_opt_str!(
            a.time.get("accepted").map(|v| output::time_full(*v).0),
            qdialog.i_time_accepted,
            ""
        );
        set_opt_str!(
            a.time.get("pending").map(|v| output::time_full(*v).0),
            qdialog.i_time_pending,
            ""
        );
        set_opt_str!(
            a.time.get("running").map(|v| output::time_full(*v).0),
            qdialog.i_time_running,
            ""
        );
        set_opt_str!(elapsed.map(|v| v.to_string()), qdialog.i_time_elapsed, "");
        if a.finished {
            let mut c = a.status.chars();
            qdialog
                .label_time_result
                .set_text(&qs(c.next().map_or_else(String::new, |f| {
                    f.to_uppercase().collect::<String>() + c.as_str()
                })));
            set_opt_str!(
                a.time.get(&a.status).map(|v| output::time_full(*v).0),
                qdialog.i_time_result,
                ""
            );
            self.stop_watcher();
        }
        Ok(())
    }
}

impl NonModalInfoDialog for DialogActionWatch {
    unsafe fn widget(&self) -> Ptr<QWidget> {
        self.qdialog.widget.as_ptr()
    }
    fn btn_close(&self) -> &QPushButton {
        &self.qdialog.btn_close
    }
    unsafe fn push(&self, data: EResult<Value>) {
        match data {
            Ok(v) => {
                if let Err(e) = self.process_data(v) {
                    eprintln!("{}", e);
                }
            }
            Err(e) => {
                eprintln!("{}", e);
            }
        }
    }
    unsafe fn close(&self) {
        self.qdialog.widget.close();
    }
}

pub struct DialogItemWatch {
    qdialog: QDialogItemWatch,
    node: String,
    oid: OID,
    arch: Mutex<Vec<ItemWatchData>>,
    _chart_view: QBox<QChartView>,
    chart: QBox<QChart>,
    series: QBox<QLineSeries>,
}

#[ui_form("../ui/item_watch.ui")]
struct QDialogItemWatch {
    pub(crate) widget: QBox<QWidget>,
    i_prop: QPtr<QComboBox>,
    i_interval: QPtr<QDoubleSpinBox>,
    i_timeframe: QPtr<QSpinBox>,
    label_oid: QPtr<QLabel>,
    btn_close: QPtr<QPushButton>,
    label_time: QPtr<QLabel>,
    label_status: QPtr<QLabel>,
    label_value: QPtr<QLabel>,
    workspace: QPtr<QGridLayout>,
    te: QPtr<QWidget>,
}

struct ItemWatchData {
    state: ItemInfo,
    dt: DateTime<Local>,
}

impl DialogItemWatch {
    pub unsafe fn new(node: &str, oid: &OID) -> Self {
        let qdialog = QDialogItemWatch::load();
        let title = qs(format!("{}/{}", node, oid));
        qdialog.widget.set_window_title(&title);
        qdialog.label_oid.set_text(&title);
        qdialog.i_prop.set_current_text(&qs("value"));
        qdialog.label_time.set_text(&qs(""));
        qdialog.label_status.set_text(&qs(""));
        qdialog.label_value.set_text(&qs(""));
        qdialog.btn_close.set_enabled(false);
        qdialog.btn_close.hide();
        qdialog.te.hide();
        let series = QLineSeries::new_0a();
        //series.set_name(&qs("Value"));
        let chart = QChart::new_0a();
        chart.add_series(&series);
        chart.create_default_axes();
        chart.legend().close();
        let chart_view = QChartView::from_q_chart(&chart);
        chart_view.resize_1a(&qdialog.te.size());
        chart_view.set_render_hint_1a(RenderHint::Antialiasing);
        chart_view.show();
        qdialog.workspace.add_widget(&chart_view);
        qdialog.i_prop.set_focus_0a();
        Self {
            qdialog,
            node: node.to_owned(),
            oid: oid.clone(),
            arch: <_>::default(),
            _chart_view: chart_view,
            chart,
            series,
        }
    }
    pub unsafe fn init(self: &Rc<Self>, u: uuid::Uuid) {
        let this = self.clone();
        self.qdialog
            .i_interval
            .value_changed()
            .connect(&SlotOfDouble::new(&self.qdialog.widget, move |val| {
                let _r = bus::call::<()>(Arc::new(NitData::stop_watcher(u)));
                let _r = bus::call::<()>(Arc::new(NitData::start_item_watcher(
                    u,
                    &this.node,
                    this.oid.clone(),
                    Duration::from_secs_f64(val),
                )));
            }));
    }
    pub unsafe fn show(self: &Rc<Self>) {
        self.qdialog.widget.show();
    }
    #[allow(clippy::cast_sign_loss)]
    unsafe fn process_data(&self, data: Value) -> EResult<()> {
        let mut state_list: Vec<ItemInfo> = Vec::deserialize(data)?;
        let state = state_list
            .pop()
            .ok_or_else(|| Error::invalid_data("invalid item state"))?;
        if state.oid != self.oid {
            return Err(Error::invalid_data("invalid item state (OID mismatch)"));
        }
        let dt = Local::now();
        self.qdialog
            .label_time
            .set_text(&qs(dt.to_rfc3339_opts(SecondsFormat::Secs, false)));
        self.qdialog.label_status.set_text(&qs(state
            .status
            .map_or_else(String::new, |v| v.to_string())));
        self.qdialog.label_value.set_text(&qs(
            crate::output::format_value(state.value.clone()).to_string()
        ));
        let mut arch = self.arch.lock().unwrap();
        arch.push(ItemWatchData { state, dt });
        let mut tf_size = self.qdialog.i_timeframe.value();
        if tf_size <= 0 {
            tf_size = 1;
        }
        let to_keep = chrono::Duration::from_std(Duration::from_secs(tf_size as u64))
            .map_err(Error::invalid_data)?;
        arch.retain(|v| dt - v.dt <= to_keep);
        self.series.clear();
        let process_status = self.qdialog.i_prop.current_text().to_std_string() == "status";
        for st in arch.iter() {
            let x = (dt - st.dt)
                .to_std()
                .map_err(Error::invalid_data)?
                .as_secs_f64()
                .floor()
                * -1.0;
            if process_status {
                if let Some(status) = st.state.status {
                    self.series.append_2_double(x, f64::from(status));
                }
            } else if let Some(ref val) = st.state.value {
                if let Ok(y) = TryInto::<f64>::try_into(val) {
                    self.series.append_2_double(x, y);
                }
            }
        }
        if process_status {
            self.series.set_color(&QColor::from_rgb_3a(0xfa, 0xb2, 0));
        } else {
            self.series.set_color(&QColor::from_rgb_3a(0, 0, 0xfa));
        }
        self.chart.remove_series(&self.series);
        self.chart.add_series(&self.series);
        self.chart.create_default_axes();
        self.chart.axis_x_0a().set_range(
            &QVariant::from_double(f64::from(-tf_size + 1)),
            &QVariant::from_double(0.0),
        );
        Ok(())
    }
}

impl NonModalInfoDialog for DialogItemWatch {
    unsafe fn widget(&self) -> Ptr<QWidget> {
        self.qdialog.widget.as_ptr()
    }
    fn btn_close(&self) -> &QPushButton {
        &self.qdialog.btn_close
    }
    unsafe fn push(&self, data: EResult<Value>) {
        match data {
            Ok(v) => {
                if let Err(e) = self.process_data(v) {
                    eprintln!("{}", e);
                }
            }
            Err(e) => {
                eprintln!("{}", e);
            }
        }
    }
    unsafe fn close(&self) {
        self.qdialog.widget.close();
    }
}

#[ui_form("../ui/svc_call.ui")]
struct QDialogSvcCall {
    pub(crate) widget: QBox<QWidget>,
    pub(crate) action_copy: QPtr<QAction>,
    tabs: QPtr<QTabWidget>,
    tabs_result: QPtr<QTabWidget>,
    btn_call: QPtr<QPushButton>,
    btn_clear: QPtr<QPushButton>,
    btn_close: QPtr<QPushButton>,
    i_svc_id: QPtr<QLineEdit>,
    i_svc_author: QPtr<QLineEdit>,
    i_svc_description: QPtr<QLineEdit>,
    i_svc_version: QPtr<QLineEdit>,
    i_custom_method: QPtr<QLineEdit>,
    i_payload: QPtr<QPlainTextEdit>,
    i_method: QPtr<QComboBox>,
    gl_params: QPtr<QGridLayout>,
    status: QPtr<QLabel>,
    splitter: QPtr<QSplitter>,
    tbl_result: QPtr<QTableWidget>,
    json_result: QPtr<QPlainTextEdit>,
}

impl QDialogSvcCall {
    unsafe fn clear_result(&self) {
        self.tbl_result.set_row_count(0);
        self.tbl_result.set_column_count(0);
        self.json_result.clear();
    }
    #[allow(clippy::too_many_lines)]
    unsafe fn svc_call(
        &self,
        u: &Mutex<Option<uuid::Uuid>>,
        items: &Mutex<Option<Vec<crate::smart_table::Item>>>,
        id: &str,
        node: &str,
        params: &Mutex<Vec<SvcCallParam>>,
    ) {
        macro_rules! err {
            ($msg: expr) => {
                items.lock().unwrap().take();
                self.clear_result();
                self.tabs_result.set_current_index(0);
                self.error(&$msg);
                return;
            };
        }
        self.set_status("Running...");
        let (method, payload) = if self.tabs.current_index() == 0 {
            let p = params.lock().unwrap();
            (
                self.i_method.current_text().to_std_string(),
                if p.is_empty() {
                    None
                } else {
                    let mut payload: BTreeMap<Value, Value> = <_>::default();
                    for param in p.iter() {
                        let name = param.name.clone();
                        let val_str = param.value.text().to_std_string();
                        if !val_str.is_empty() {
                            match param.kind.current_text().to_std_string().as_str() {
                                "float" => match val_str.parse::<f64>() {
                                    Ok(val) => {
                                        payload.insert(Value::String(name), Value::F64(val));
                                    }
                                    Err(e) => {
                                        err!(format!("Unable to parse the param {}: {}", name, e));
                                    }
                                },
                                "integer" => match val_str.parse::<u64>() {
                                    Ok(val) => {
                                        payload.insert(Value::String(name), Value::U64(val));
                                    }
                                    Err(_) => match val_str.parse::<i64>() {
                                        Ok(val) => {
                                            payload.insert(Value::String(name), Value::I64(val));
                                        }
                                        Err(e) => {
                                            err!(format!(
                                                "Unable to parse the param {}: {}",
                                                name, e
                                            ));
                                        }
                                    },
                                },
                                "string" => {
                                    payload.insert(Value::String(name), Value::String(val_str));
                                }
                                "JSON" => match serde_json::from_str(&val_str) {
                                    Ok(val) => {
                                        payload.insert(Value::String(name), val);
                                    }
                                    Err(e) => {
                                        err!(format!("Unable to parse the param {}: {}", name, e));
                                    }
                                },
                                _ => match val_str.parse::<Value>() {
                                    Ok(val) => {
                                        payload.insert(Value::String(name), val);
                                    }
                                    Err(e) => {
                                        err!(format!("Unable to parse the param {}: {}", name, e));
                                    }
                                },
                            }
                        }
                    }
                    Some(Value::Map(payload))
                },
            )
        } else {
            let pl = self.i_payload.to_plain_text().to_std_string();
            let payload = pl.trim();
            (
                self.i_custom_method.text().to_std_string(),
                if payload.is_empty() {
                    None
                } else {
                    Some(match serde_yaml::from_str::<Value>(payload) {
                        Ok(v) => v,
                        Err(e) => {
                            err!(format!("Unable to parse payload: {e}"));
                        }
                    })
                },
            )
        };
        if let Some(u) = u.lock().unwrap().as_ref() {
            let nit = Arc::new(NitData::new_svc_call(
                *u,
                node,
                id.to_owned(),
                method,
                payload,
            ));
            let _r = bus::call::<()>(nit);
        } else {
            err!("dialog not registered");
        }
    }
    unsafe fn set_status(&self, text: &str) {
        self.status.set_text(&qs(text));
    }
    unsafe fn error(&self, text: &str) {
        self.status.set_text(&qs(format!(
            "<span style=\"color: red; font-weight: bold\">{text}</span>"
        )));
    }
}

pub struct DialogSvcCall {
    qdialog: Rc<QDialogSvcCall>,
    _id: String,
    _node: String,
    _info: Rc<SvcInfo>,
    _params: Rc<Mutex<Vec<SvcCallParam>>>,
    op: Rc<Mutex<Option<Instant>>>,
    u: Rc<Mutex<Option<uuid::Uuid>>>,
    items: Rc<Mutex<Option<Vec<crate::smart_table::Item>>>>,
}

struct SvcCallParam {
    name: String,
    _label: QBox<QLabel>,
    value: QBox<QLineEdit>,
    kind: QBox<QComboBox>,
}

impl StaticUpcast<QObject> for DialogSvcCall {
    unsafe fn static_upcast(ptr: Ptr<Self>) -> Ptr<QObject> {
        ptr.qdialog.widget.as_ptr().static_upcast()
    }
}

impl DialogSvcCall {
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_possible_wrap)]
    #[allow(clippy::too_many_lines)]
    pub unsafe fn new(id: &str, node: &str, info: SvcInfo) -> Rc<Self> {
        let dialog = Rc::new(QDialogSvcCall::load());
        let op: Rc<Mutex<Option<Instant>>> = <_>::default();
        //dialog.tbl_result.hide();
        let info = Rc::new(info);
        let u: Rc<Mutex<Option<uuid::Uuid>>> = <_>::default();
        let params: Rc<Mutex<Vec<SvcCallParam>>> = <_>::default();
        let items: Rc<Mutex<Option<Vec<crate::smart_table::Item>>>> = <_>::default();
        dialog
            .widget
            .set_window_title(&qs(format!("Service call {node}/{id}")));
        dialog.i_svc_id.set_text(&qs(id));
        set_opt_str!(&info.author, dialog.i_svc_author, "");
        set_opt_str!(&info.description, dialog.i_svc_description, "");
        set_opt_str!(&info.version, dialog.i_svc_version, "");
        let d = dialog.clone();
        let p = params.clone();
        let svc_id = id.to_owned();
        let svc_node = node.to_owned();
        let op_c = op.clone();
        let u_c = u.clone();
        let items_c = items.clone();
        dialog
            .btn_call
            .clicked()
            .connect(&SlotNoArgs::new(&dialog.widget, move || {
                op_c.lock().unwrap().replace(Instant::now());
                d.svc_call(&u_c, &items_c, &svc_id, &svc_node, &p);
            }));
        let d = dialog.clone();
        let p = params.clone();
        dialog
            .btn_clear
            .clicked()
            .connect(&SlotNoArgs::new(&dialog.widget, move || {
                d.i_custom_method.clear();
                d.i_payload.clear();
                for param in p.lock().unwrap().iter() {
                    param.value.clear();
                    param.kind.set_current_text(&qs("auto"));
                }
            }));
        dialog.i_method.clear();
        dialog.i_method.add_item_q_string(&qs("test"));
        for method in info.methods.keys() {
            dialog.i_method.add_item_q_string(&qs(method));
        }
        let i = info.clone();
        let p = params.clone();
        let d = dialog.clone();
        dialog.status.clear();
        dialog
            .i_method
            .activated2()
            .connect(&SlotNoArgs::new(&dialog.widget, move || {
                let mut par = p.lock().unwrap();
                while d.gl_params.count() > 1 {
                    let widget = d.gl_params.item_at(0).widget();
                    if !widget.is_null() {
                        widget.hide();
                    }
                    d.gl_params.remove_widget(widget);
                }
                let spacer = d.gl_params.take_at(0);
                par.clear();
                if let Some(method_info) = i.methods.get(&d.i_method.current_text().to_std_string())
                {
                    let mut params = method_info
                        .params
                        .iter()
                        .map(|(k, v)| (k.as_str(), v))
                        .collect::<Vec<(&str, &SvcMethodInfoParam)>>();
                    params.sort_by_key(|k| !k.1.required);
                    for (row, (name, val)) in params.into_iter().enumerate() {
                        let label = QLabel::new();
                        if val.required {
                            label.set_text(&qs(format!(
                                "<span style=\"font-weight: bold\">{name}</span>"
                            )));
                        } else {
                            label.set_text(&qs(name));
                        }
                        let value = QLineEdit::new();
                        let kind = QComboBox::new_0a();
                        kind.add_item_q_string(&qs("auto"));
                        kind.add_item_q_string(&qs("float"));
                        kind.add_item_q_string(&qs("integer"));
                        kind.add_item_q_string(&qs("string"));
                        kind.add_item_q_string(&qs("JSON"));
                        d.gl_params.add_widget_3a(&label, row as i32, 0);
                        d.gl_params.add_widget_3a(&value, row as i32, 1);
                        d.gl_params.add_widget_3a(&kind, row as i32, 2);
                        par.push(SvcCallParam {
                            name: name.to_owned(),
                            _label: label,
                            value,
                            kind,
                        });
                    }
                }
                d.gl_params.add_item(spacer);
            }));
        let this = Self {
            qdialog: dialog,
            _id: id.to_owned(),
            _node: node.to_owned(),
            _info: info,
            _params: params,
            op,
            u,
            items,
        };
        let this = Rc::new(this);
        let keybind = QKeySequence::key_bindings(StandardKey::Copy).take_first();
        this.qdialog.action_copy.set_shortcut(keybind.as_ref());
        this.qdialog
            .action_copy
            .triggered()
            .connect(&this.slot_on_copy());
        this.qdialog.widget.add_action(&this.qdialog.action_copy);
        this
    }
    pub fn set_uuid(&self, u: uuid::Uuid) {
        self.u.lock().unwrap().replace(u);
    }
    pub unsafe fn show(&self) {
        self.qdialog.widget.show();
        if let Some((size_left, size_right)) = splitter_sizes(&self.qdialog.splitter) {
            let size_total = size_left + size_right;
            let size_left = size_total / 3;
            let size_right = size_total / 3 * 2;
            self.qdialog
                .splitter
                .set_sizes(&new_size(size_left, size_right));
        }
    }
    #[allow(clippy::too_many_lines)]
    pub unsafe fn process_data(&self, data: EResult<Value>) {
        self.items.lock().unwrap().take();
        self.qdialog.clear_result();
        match data {
            Ok(v) => {
                if let Some(o) = self.op.lock().unwrap().as_ref() {
                    self.qdialog
                        .set_status(&format!("elapsed: {:?}", o.elapsed()));
                } else {
                    self.qdialog.set_status("ok");
                }
                match serde_json::to_string_pretty(&v) {
                    Ok(v) => self.qdialog.json_result.set_plain_text(&qs(v)),
                    Err(e) => self
                        .qdialog
                        .json_result
                        .set_plain_text(&qs(format!("JSON error: {e}"))),
                }
                match v {
                    Value::Seq(val) => {
                        if !val.is_empty() {
                            let cols: Vec<String>;
                            let t_cols;
                            let t = if let Value::Map(m) = &val[0] {
                                cols = m.keys().map(ToString::to_string).collect();
                                t_cols = cols.iter().map(String::as_str).collect::<Vec<&str>>();
                                let mut t = Table::new(&t_cols);
                                for row in &val {
                                    if let Value::Map(m) = row {
                                        t.append_row(m.values().map(FormattedValue::new).collect());
                                    } else {
                                        t.append_row(vec![FormattedValue::new(row)]);
                                    }
                                }
                                t
                            } else {
                                let mut t = Table::new(&[" "]);
                                for row in &val {
                                    t.append_row(vec![FormattedValue::new(row)]);
                                }
                                t
                            };
                            self.items
                                .lock()
                                .unwrap()
                                .replace(t.fill_qt(&self.qdialog.tbl_result));
                        }
                    }
                    Value::Map(val) => {
                        let mut t = Table::new(&["name", "value"]);
                        for (k, v) in &val {
                            t.append_row(vec![FormattedValue::new(k), FormattedValue::new(v)]);
                        }
                        self.items
                            .lock()
                            .unwrap()
                            .replace(t.fill_qt(&self.qdialog.tbl_result));
                    }
                    _ => {
                        let mut t = Table::new(&[" "]);
                        let ok;
                        if v == Value::Unit {
                            ok = Value::String("OK".to_owned());
                            t.append_row(vec![FormattedValue {
                                color: FormattedValueColor::Green,
                                value: &ok,
                            }]);
                        } else {
                            t.append_row(vec![FormattedValue::new(&v)]);
                        }
                        self.items
                            .lock()
                            .unwrap()
                            .replace(t.fill_qt(&self.qdialog.tbl_result));
                    }
                }
            }
            Err(e) => {
                self.qdialog.tabs_result.set_current_index(0);
                self.qdialog.error(&format!("{} ({})", e.kind(), e.code()));
                let mut t = Table::new(&["error", "code", "message"]);
                let kind = Value::String(e.kind().to_string());
                let code = Value::I16(e.code());
                let message = Value::String(e.message().unwrap_or_default().to_owned());
                t.append_row(vec![
                    FormattedValue {
                        color: FormattedValueColor::Red,
                        value: &kind,
                    },
                    FormattedValue {
                        color: FormattedValueColor::Red,
                        value: &code,
                    },
                    FormattedValue {
                        color: FormattedValueColor::Red,
                        value: &message,
                    },
                ]);
                self.items
                    .lock()
                    .unwrap()
                    .replace(t.fill_qt(&self.qdialog.tbl_result));
            }
        }
    }
    #[slot(SlotNoArgs)]
    unsafe fn on_copy(self: &Rc<Self>) {
        let result = copy_from_table(&[&self.qdialog.tbl_result]);
        if !result.is_empty() {
            let mut clipboard = Clipboard::new().unwrap();
            clipboard.set_text(result).unwrap();
        }
    }
}

impl NonModalInfoDialog for DialogSvcCall {
    unsafe fn widget(&self) -> Ptr<QWidget> {
        self.qdialog.widget.as_ptr()
    }
    fn btn_close(&self) -> &QPushButton {
        &self.qdialog.btn_close
    }
    unsafe fn push(&self, data: EResult<Value>) {
        self.process_data(data);
    }
    unsafe fn close(&self) {
        self.qdialog.widget.close();
    }
}
