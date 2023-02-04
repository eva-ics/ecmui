use crate::common::{
    spent_time, ActionRecord, BrokerInfo, ItemInfo, LogRecord, Nit, NitKind, NodeInfo, SPointInfo,
    SvcData,
};
use crate::smart_table::{self, FormattedValue, FormattedValueColor};
use crate::ui::Ui;
use chrono::{DateTime, Local, NaiveDateTime, SecondsFormat, Utc};
use eva_common::prelude::*;
use serde::Deserialize;
use std::collections::{btree_map, BTreeMap};
use std::rc::Rc;

#[allow(clippy::cast_possible_truncation)]
pub fn time_str(ts: f64) -> (String, i64) {
    let ts_i = ts.floor() as i64;
    let dt_utc = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(ts_i, 0), Utc);
    let dt: DateTime<Local> = DateTime::from(dt_utc);
    (dt.to_rfc3339_opts(SecondsFormat::Secs, false), ts_i)
}

#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
pub fn time_full(ts: f64) -> (String, i64) {
    let ts_i = ts.trunc() as i64;
    let dt_utc = DateTime::<Utc>::from_utc(
        NaiveDateTime::from_timestamp(ts_i, (ts.fract() * 1_000_000_000.0) as u32),
        Utc,
    );
    let dt: DateTime<Local> = DateTime::from(dt_utc);
    (dt.to_rfc3339_opts(SecondsFormat::Micros, true), ts_i)
}

#[inline]
pub fn format_value(val: Option<Value>) -> Value {
    format_val(val, false)
}

#[inline]
pub fn format_value_pretty(val: Option<Value>) -> Value {
    format_val(val, true)
}

fn format_val(val: Option<Value>, pretty: bool) -> Value {
    if let Some(value) = val {
        match value {
            Value::Seq(_) | Value::Map(_) => {
                let s = if pretty {
                    serde_json::to_string_pretty(&value)
                } else {
                    serde_json::to_string(&value)
                };
                Value::String(s.unwrap_or_else(|_| "<obj>".to_owned()))
            }
            _ => value.to_no_bytes(),
        }
    } else {
        Value::Unit
    }
}

#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
pub unsafe fn result(ui: &Rc<Ui>, nit: Nit, value: Value) -> EResult<()> {
    macro_rules! err_data {
        () => {
            return Err(Error::invalid_data("invalid value received"));
        };
    }
    match nit.kind() {
        NitKind::State => {
            if let Value::Seq(seq) = value {
                if seq.len() == 2 {
                    let mut i = seq.into_iter();
                    let mut data: BTreeMap<String, Value> =
                        BTreeMap::deserialize(i.next().unwrap())?;
                    if let btree_map::Entry::Occupied(mut entry) = data.entry("time".to_owned()) {
                        if let Value::F64(ts) = entry.get() {
                            let (tss, ts_i) = time_str(*ts);
                            entry.insert(Value::String(format!("{} ({})", tss, ts_i)));
                        }
                    }
                    if let btree_map::Entry::Occupied(mut entry) = data.entry("uptime".to_owned()) {
                        if let Value::F64(time) = entry.get() {
                            let ts_i = time.floor() as u64;
                            let t_str = spent_time(ts_i);
                            entry.insert(Value::String(format!("{} ({})", t_str, ts_i)));
                        }
                    }
                    name_value(ui, data, true);
                    state_node_list(ui, Vec::deserialize(i.next().unwrap())?);
                    Ok(())
                } else {
                    err_data!();
                }
            } else {
                err_data!();
            }
        }
        NitKind::Services => {
            list_services(ui, Vec::deserialize(value)?);
            Ok(())
        }
        NitKind::SPoints => {
            list_spoints(ui, Vec::deserialize(value)?);
            Ok(())
        }
        NitKind::Items(_, _) => {
            list_items(ui, Vec::deserialize(value)?);
            Ok(())
        }
        NitKind::Broker => {
            list_broker_clients(ui, BrokerInfo::deserialize(value)?);
            Ok(())
        }
        NitKind::Log(_) => {
            output_log(ui, Vec::deserialize(value)?);
            Ok(())
        }
        NitKind::Actions(_) => {
            output_actions(ui, Vec::deserialize(value)?);
            Ok(())
        }
        _ => Ok(()),
    }
}
#[allow(clippy::cast_possible_wrap)]
#[allow(clippy::cast_possible_truncation)]
pub unsafe fn name_value(ui: &Ui, data: BTreeMap<String, Value>, primary_table: bool) {
    ui.clear_primary_table();
    let qt_table = if primary_table {
        ui.clear_primary_table();
        &ui.window.primary_table
    } else {
        ui.clear_secondary_table();
        &ui.window.secondary_table
    };
    let mut smart_table = smart_table::Table::new(&["name", "value"]);
    let mut keys = data.keys().map(String::as_str).collect::<Vec<&str>>();
    keys.sort_unstable();
    let key_vals = keys
        .iter()
        .map(|v| Value::String((*v).to_owned()))
        .collect::<Vec<Value>>();
    for (key_val, key) in key_vals.iter().zip(keys) {
        smart_table.append_row(vec![key_val.into(), data.get(key).unwrap().into()]);
    }
    let mut items = smart_table.fill_qt(qt_table);
    ui.primary_table_items.lock().unwrap().append(&mut items);
}

unsafe fn state_node_list(ui: &Rc<Ui>, data: Vec<NodeInfo>) {
    ui.clear_secondary_table();
    let qt_table = &ui.window.secondary_table;
    let mut smart_table =
        smart_table::Table::new(&["node", "svc", "type", "online", "version", "build"]);
    let mut rows: Vec<Vec<(Value, FormattedValueColor)>> = Vec::new();
    for d in data {
        let mut row: Vec<(Value, FormattedValueColor)> =
            vec![(Value::String(d.name), FormattedValueColor::Normal)];
        if let Some(svc) = d.svc {
            row.push((Value::String(svc), FormattedValueColor::Normal));
        } else {
            row.push((Value::Unit, FormattedValueColor::Normal));
        }
        let kind = if d.remote { "remote" } else { "local" };
        row.push((Value::String(kind.to_owned()), FormattedValueColor::Normal));
        let (status, color) = if d.online {
            ("online", FormattedValueColor::Green)
        } else {
            ("offline", FormattedValueColor::Gray)
        };
        row.push((Value::String(status.to_owned()), color));
        if let Some(info) = d.info {
            row.push((Value::String(info.version), FormattedValueColor::Normal));
            row.push((Value::U64(info.build), FormattedValueColor::Normal));
        } else {
            row.push((Value::Unit, FormattedValueColor::Normal));
            row.push((Value::Unit, FormattedValueColor::Normal));
        }
        rows.push(row);
    }
    for row in &rows {
        smart_table.append_row(
            row.iter()
                .map(|(value, color)| FormattedValue {
                    value,
                    color: *color,
                })
                .collect::<Vec<FormattedValue>>(),
        );
    }
    let mut items = smart_table.fill_qt(qt_table);
    ui.secondary_table_items.lock().unwrap().append(&mut items);
}

unsafe fn list_broker_clients(ui: &Rc<Ui>, data: BrokerInfo) {
    ui.clear_tables();
    ui.window.secondary_table.hide();
    let qt_table = &ui.window.primary_table;
    let mut smart_table = smart_table::Table::new(&[
        "name",
        "type",
        "source",
        "port",
        "r_frames",
        "w_frames",
        "r_bytes",
        "w_bytes",
        "queue",
        "instances",
    ]);
    let mut rows: Vec<Vec<Value>> = Vec::new();
    {
        let client_name = crate::CLIENT_NAME.lock().unwrap();
        for d in data.clients {
            if let Some(n) = client_name.as_ref() {
                if n == &d.name {
                    continue;
                }
            }
            let mut row: Vec<Value> = vec![Value::String(d.name), Value::String(d.kind)];
            if let Some(src) = d.source {
                row.push(Value::String(src));
            } else {
                row.push(Value::Unit);
            }
            if let Some(port) = d.port {
                row.push(Value::String(port));
            } else {
                row.push(Value::Unit);
            }
            row.push(Value::U64(d.r_frames));
            row.push(Value::U64(d.w_frames));
            row.push(Value::U64(d.r_bytes));
            row.push(Value::U64(d.w_bytes));
            row.push(Value::U64(d.queue));
            row.push(Value::U64(d.instances));
            rows.push(row);
        }
    }
    for row in &rows {
        smart_table.append_row(row.iter().map(Into::into).collect::<Vec<FormattedValue>>());
    }
    let mut items = smart_table.fill_qt(qt_table);
    ui.primary_table_items.lock().unwrap().append(&mut items);
}

unsafe fn list_services(ui: &Rc<Ui>, data: Vec<SvcData>) {
    ui.clear_tables();
    ui.window.secondary_table.hide();
    let qt_table = &ui.window.primary_table;
    let mut smart_table = smart_table::Table::new(&["id", "status", "pid", "launcher"]);
    let mut rows: Vec<Vec<(Value, FormattedValueColor)>> = Vec::new();
    for d in data {
        let mut row: Vec<(Value, FormattedValueColor)> =
            vec![(Value::String(d.id), FormattedValueColor::Normal)];
        if d.status == "online" {
            row.push((Value::String(d.status), FormattedValueColor::Green));
        } else {
            row.push((Value::String(d.status), FormattedValueColor::Gray));
        }
        if let Some(pid) = d.pid {
            row.push((Value::U32(pid), FormattedValueColor::Normal));
        } else {
            row.push((Value::Unit, FormattedValueColor::Normal));
        }
        row.push((Value::String(d.launcher), FormattedValueColor::Normal));
        rows.push(row);
    }
    for row in &rows {
        smart_table.append_row(
            row.iter()
                .map(|(value, color)| FormattedValue {
                    value,
                    color: *color,
                })
                .collect::<Vec<FormattedValue>>(),
        );
    }
    let mut items = smart_table.fill_qt(qt_table);
    ui.primary_table_items.lock().unwrap().append(&mut items);
}

unsafe fn list_items(ui: &Rc<Ui>, data: Vec<ItemInfo>) {
    ui.clear_tables();
    ui.window.secondary_table.hide();
    let qt_table = &ui.window.primary_table;
    let mut smart_table = smart_table::Table::new(&[
        "oid",
        "enabled",
        "connected",
        "status",
        "value",
        "set time",
        "node",
    ]);
    let mut rows: Vec<Vec<(Value, FormattedValueColor)>> = Vec::new();
    for d in data {
        let color = if d.connected {
            FormattedValueColor::Normal
        } else {
            FormattedValueColor::Gray
        };
        let row: Vec<(Value, FormattedValueColor)> = vec![
            (Value::String(d.oid.as_str().to_owned()), color),
            (Value::Bool(d.enabled), color),
            (Value::Bool(d.connected), color),
            (
                if let Some(status) = d.status {
                    Value::I16(status)
                } else {
                    Value::Unit
                },
                color,
            ),
            (format_value(d.value), color),
            (
                if let Some(t) = d.t {
                    Value::String(time_str(t).0)
                } else {
                    Value::Unit
                },
                color,
            ),
            (Value::String(d.node), color),
        ];
        rows.push(row);
    }
    for row in &rows {
        smart_table.append_row(
            row.iter()
                .map(|(value, color)| FormattedValue {
                    value,
                    color: *color,
                })
                .collect::<Vec<FormattedValue>>(),
        );
    }
    let mut items = smart_table.fill_qt(qt_table);
    ui.primary_table_items.lock().unwrap().append(&mut items);
}

unsafe fn list_spoints(ui: &Rc<Ui>, data: Vec<SPointInfo>) {
    ui.clear_tables();
    ui.window.secondary_table.hide();
    let qt_table = &ui.window.primary_table;
    let mut smart_table = smart_table::Table::new(&["name", "source", "port", "version", "build"]);
    let mut rows: Vec<Vec<Value>> = Vec::new();
    {
        let client_name = crate::CLIENT_NAME.lock().unwrap();
        for d in data {
            if let Some(n) = client_name.as_ref() {
                if n == &d.name {
                    continue;
                }
            }
            let row: Vec<Value> = vec![
                Value::String(d.name),
                Value::String(d.source),
                Value::String(d.port),
                Value::String(d.info.version),
                Value::U64(d.info.build),
            ];
            rows.push(row);
        }
    }
    for row in &rows {
        smart_table.append_row(row.iter().map(Into::into).collect::<Vec<FormattedValue>>());
    }
    let mut items = smart_table.fill_qt(qt_table);
    ui.primary_table_items.lock().unwrap().append(&mut items);
}

unsafe fn output_log(ui: &Rc<Ui>, data: Vec<LogRecord>) {
    ui.clear_tables();
    ui.window.secondary_table.hide();
    let qt_table = &ui.window.primary_table;
    let mut smart_table = smart_table::Table::new(&["time", "level", "module", "message"]);
    let mut rows: Vec<Vec<(Value, FormattedValueColor)>> = Vec::new();
    for d in data.into_iter().rev() {
        let color = match d.l {
            eva_common::LOG_LEVEL_TRACE => FormattedValueColor::DarkGray,
            eva_common::LOG_LEVEL_DEBUG => FormattedValueColor::Gray,
            eva_common::LOG_LEVEL_WARN => FormattedValueColor::Orange,
            eva_common::LOG_LEVEL_ERROR => FormattedValueColor::Red,
            _ => FormattedValueColor::Normal,
        };
        let row: Vec<(Value, FormattedValueColor)> = vec![
            (Value::String(d.dt), color),
            (Value::String(d.lvl), color),
            (
                if let Some(module) = d.module {
                    Value::String(module)
                } else {
                    Value::Unit
                },
                color,
            ),
            (
                if let Some(msg) = d.msg {
                    Value::String(msg)
                } else {
                    Value::Unit
                },
                color,
            ),
        ];
        rows.push(row);
    }
    for row in &rows {
        smart_table.append_row(
            row.iter()
                .map(|(value, color)| FormattedValue {
                    value,
                    color: *color,
                })
                .collect::<Vec<FormattedValue>>(),
        );
    }
    let mut items = smart_table.fill_qt(qt_table);
    ui.primary_table_items.lock().unwrap().append(&mut items);
}

unsafe fn output_actions(ui: &Rc<Ui>, data: Vec<ActionRecord>) {
    ui.clear_tables();
    ui.window.secondary_table.hide();
    let qt_table = &ui.window.primary_table;
    let mut smart_table =
        smart_table::Table::new(&["time", "uuid", "oid", "status", "elapsed", "node", "svc"]);
    let mut rows: Vec<Vec<(Value, FormattedValueColor)>> = Vec::new();
    for d in data.into_iter().rev() {
        let color = FormattedValueColor::from_action_status(&d.status);
        let elapsed = d.elapsed();
        let row: Vec<(Value, FormattedValueColor)> = vec![
            (
                Value::String(d.time().map_or_else(String::new, |t| time_full(t).0)),
                FormattedValueColor::Normal,
            ),
            (
                Value::String(d.uuid.to_string()),
                FormattedValueColor::Normal,
            ),
            (Value::String(d.oid), FormattedValueColor::Normal),
            (Value::String(d.status), color),
            (
                elapsed.map_or(Value::Unit, Value::F64),
                FormattedValueColor::Normal,
            ),
            (Value::String(d.node), FormattedValueColor::Normal),
            (Value::String(d.svc), FormattedValueColor::Normal),
        ];
        rows.push(row);
    }
    for row in &rows {
        smart_table.append_row(
            row.iter()
                .map(|(value, color)| FormattedValue {
                    value,
                    color: *color,
                })
                .collect::<Vec<FormattedValue>>(),
        );
    }
    let mut items = smart_table.fill_qt(qt_table);
    ui.primary_table_items.lock().unwrap().append(&mut items);
}
