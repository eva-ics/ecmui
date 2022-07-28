use cpp_core::CppBox;
use eva_common::prelude::*;
use qt_core::{qs, QPtr, QVariant};
use qt_gui::{QBrush, QColor};
use qt_widgets::{QTableWidget, QTableWidgetItem};
use std::os::raw::c_int;

pub struct Item {
    _item: CppBox<QTableWidgetItem>,
}

struct SmartCol<'a> {
    name: &'a str,
}

pub struct FormattedValue<'a> {
    pub color: FormattedValueColor,
    pub value: &'a Value,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum FormattedValueColor {
    Normal,
    Red,
    Orange,
    Green,
    Gray,
    DarkGray,
}

impl FormattedValueColor {
    pub fn rich(self, s: &str, xtra: Option<&str>) -> String {
        match self {
            FormattedValueColor::Green => {
                format!(
                    "<span style=\"color: #009600; {}\">{}</span>",
                    xtra.unwrap_or_default(),
                    s
                )
            }
            FormattedValueColor::Red => {
                format!(
                    "<span style=\"color: #960000; {}\">{}</span>",
                    xtra.unwrap_or_default(),
                    s
                )
            }
            FormattedValueColor::Orange => {
                format!(
                    "<span style=\"color: #b86300; {}\">{}</span>",
                    xtra.unwrap_or_default(),
                    s
                )
            }
            FormattedValueColor::Gray => {
                format!(
                    "<span style=\"color: #787878; {}\">{}</span>",
                    xtra.unwrap_or_default(),
                    s
                )
            }
            FormattedValueColor::DarkGray => {
                format!(
                    "<span style=\"color: #b4b4b4; {}\">{}</span>",
                    xtra.unwrap_or_default(),
                    s
                )
            }
            FormattedValueColor::Normal => s.to_owned(),
        }
    }
    pub fn from_action_status(s: &str) -> Self {
        match s {
            "canceled" | "terminated" => FormattedValueColor::Orange,
            "failed" => FormattedValueColor::Red,
            "created" => FormattedValueColor::DarkGray,
            "pending" => FormattedValueColor::Gray,
            "completed" => FormattedValueColor::Green,
            _ => FormattedValueColor::Normal,
        }
    }
    pub unsafe fn brush(self) -> Option<CppBox<QBrush>> {
        match self {
            FormattedValueColor::Green => {
                let brush = QBrush::new();
                let color = QColor::new();
                color.set_red(0);
                color.set_green(0x96);
                color.set_blue(0);
                brush.set_color_q_color(&color);
                Some(brush)
            }
            FormattedValueColor::Red => {
                let brush = QBrush::new();
                let color = QColor::new();
                color.set_red(150);
                color.set_green(0);
                color.set_blue(0);
                brush.set_color_q_color(&color);
                Some(brush)
            }
            FormattedValueColor::Orange => {
                let brush = QBrush::new();
                let color = QColor::new();
                color.set_red(0xb8);
                color.set_green(0x63);
                color.set_blue(0);
                brush.set_color_q_color(&color);
                Some(brush)
            }
            FormattedValueColor::Gray => {
                let brush = QBrush::new();
                let color = QColor::new();
                color.set_red(0x78);
                color.set_green(0x78);
                color.set_blue(0x78);
                brush.set_color_q_color(&color);
                Some(brush)
            }
            FormattedValueColor::DarkGray => {
                let brush = QBrush::new();
                let color = QColor::new();
                color.set_red(0xb4);
                color.set_green(0xb4);
                color.set_blue(0xb4);
                brush.set_color_q_color(&color);
                Some(brush)
            }
            FormattedValueColor::Normal => None,
        }
    }
}

impl<'a> From<&'a Value> for FormattedValue<'a> {
    fn from(value: &'a Value) -> Self {
        FormattedValue {
            value,
            color: FormattedValueColor::Normal,
        }
    }
}

pub struct Table<'a> {
    cols: Vec<SmartCol<'a>>,
    data: Vec<Vec<FormattedValue<'a>>>,
}

impl<'a> Table<'a> {
    pub fn new(cols: &'a [&'a str]) -> Self {
        Self {
            cols: cols.iter().map(|name| SmartCol { name }).collect(),
            data: <_>::default(),
        }
    }
    pub fn append_row(&mut self, row: Vec<FormattedValue<'a>>) {
        self.data.push(row);
    }
    // fills QTableWidget with data
    // returns vec of Item, which MUST be kept until the table is cleared
    //
    // The table MUST be cleared before calling
    #[allow(clippy::cast_possible_wrap)]
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::too_many_lines)]
    pub unsafe fn fill_qt(&self, table: &QPtr<QTableWidget>) -> Vec<Item> {
        let max = c_int::MAX.try_into().unwrap_or_default();
        //let mut max_col_len: Vec<usize> = self.cols.iter().map(|col| col.name.len()).collect();
        let mut items = Vec::new();
        // insert columns
        for col_n in 0..self.cols.len() {
            if col_n <= max {
                table.insert_column(col_n as c_int);
            }
        }
        // insert rows and fill them with data
        for (row_n, row) in self.data.iter().enumerate() {
            if row_n <= max {
                table.insert_row(row_n as c_int);
                for col_n in 0..self.cols.len() {
                    if col_n <= max {
                        let item = QTableWidgetItem::new();
                        if let Some(val) = row.get(col_n) {
                            macro_rules! set_data {
                                ($data: expr) => {
                                    item.set_data(0, &$data)
                                };
                            }
                            match val.value {
                                Value::Bool(v) => {
                                    set_data!(QVariant::from_bool(*v));
                                }
                                Value::U8(v) => set_data!(QVariant::from_uint(u32::from(*v))),
                                Value::I8(v) => set_data!(QVariant::from_int(i32::from(*v))),
                                Value::U16(v) => set_data!(QVariant::from_uint(u32::from(*v))),
                                Value::I16(v) => set_data!(QVariant::from_int(i32::from(*v))),
                                Value::U32(v) => set_data!(QVariant::from_uint(*v)),
                                Value::I32(v) => set_data!(QVariant::from_int(*v)),
                                Value::U64(v) => set_data!(QVariant::from_u64(*v)),
                                Value::I64(v) => set_data!(QVariant::from_i64(*v)),
                                Value::F32(v) => set_data!(QVariant::from_float(*v)),
                                Value::F64(v) => set_data!(QVariant::from_double(*v)),
                                s => item.set_text(&qs(s.to_string())),
                            };
                            if let Some(brush) = val.color.brush() {
                                item.set_foreground(&brush);
                            }
                        }
                        table.set_item(row_n as c_int, col_n as c_int, &item);
                        items.push(Item { _item: item });
                    }
                }
            }
        }
        // set column names
        for (col_n, col) in self.cols.iter().enumerate() {
            if col_n <= max {
                let h = QTableWidgetItem::new();
                h.set_text(&qs(col.name));
                table.set_horizontal_header_item(col_n as c_int, &h);
                items.push(Item { _item: h });
            }
        }
        table.resize_columns_to_contents();
        table.show();
        items
    }
}
