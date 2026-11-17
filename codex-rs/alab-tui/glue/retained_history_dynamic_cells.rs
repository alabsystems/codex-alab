// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual mutable-row identities are shared across history, active registries and held views.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Data {
    call_id: String,
    state: u32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Table {
    rows: Vec<Row>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Row {
    call_id: String,
    name: String,
    arguments: Value,
    status: DynamicToolCallStatus,
    interrupted: bool,
    output: Option<Vec<String>>,
    duration_ms: Option<i64>,
}
#[derive(Default)]
pub(crate) struct Capture {
    indices: HashMap<usize, u32>,
    held: Vec<DynamicToolCallCell>,
}
impl Capture {
    pub(in crate::history_cell) fn cell(
        &mut self,
        actual: &DynamicToolCallCell,
    ) -> io::Result<Data> {
        let pointer = Arc::as_ptr(&actual.data) as usize;
        let index = if let Some(index) = self.indices.get(&pointer) {
            if self.held[*index as usize].call_id != actual.call_id {
                return Err(native::invalid(
                    "dynamic alias changed its original call identity",
                ));
            }
            *index
        } else {
            if self.held.len() >= native::MAX_NODES {
                return Err(native::invalid("dynamic row capacity"));
            }
            let index = u32::try_from(self.held.len()).map_err(native::invalid)?;
            self.held.push(actual.clone());
            self.indices.insert(pointer, index);
            index
        };
        Ok(Data {
            call_id: actual.call_id.clone(),
            state: index,
        })
    }
    pub(crate) fn export(&self) -> io::Result<Table> {
        let rows = self
            .held
            .iter()
            .map(|actual| {
                let data = actual
                    .data
                    .read()
                    .map_err(|_| native::invalid("dynamic row capture lock poisoned"))?;
                Ok(Row {
                    call_id: actual.call_id.clone(),
                    name: data.name.clone(),
                    arguments: data.arguments.clone(),
                    status: data.status.clone(),
                    interrupted: data.interrupted,
                    output: data.output.clone(),
                    duration_ms: data.duration_ms,
                })
            })
            .collect::<io::Result<_>>()?;
        Ok(Table { rows })
    }
}
#[derive(Default)]
pub(crate) struct Restored {
    rows: Vec<DynamicToolCallCell>,
    installed: bool,
}
impl Restored {
    pub(crate) fn install(&mut self, table: Table) -> io::Result<()> {
        if self.installed || table.rows.len() > native::MAX_NODES {
            return Err(native::invalid(
                "dynamic table already installed or exceeds capacity",
            ));
        }
        self.installed = true;
        self.rows = table
            .rows
            .into_iter()
            .map(|row| DynamicToolCallCell {
                call_id: row.call_id,
                data: Arc::new(RwLock::new(DynamicToolCallData {
                    name: row.name,
                    arguments: row.arguments,
                    status: row.status,
                    interrupted: row.interrupted,
                    output: row.output,
                    duration_ms: row.duration_ms,
                })),
            })
            .collect();
        Ok(())
    }
    pub(in crate::history_cell) fn cell(&self, data: &Data) -> io::Result<DynamicToolCallCell> {
        let actual = self
            .rows
            .get(data.state as usize)
            .ok_or_else(|| native::invalid("dynamic row reference missing"))?;
        if actual.call_id != data.call_id {
            return Err(native::invalid(
                "dynamic row reference changed call identity",
            ));
        }
        Ok(actual.clone())
    }
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        table: &mut Capture,
    ) -> io::Result<Option<Self>> {
        actual
            .as_any()
            .downcast_ref::<DynamicToolCallCell>()
            .map(|actual| table.cell(actual))
            .transpose()
    }
    pub(in crate::history_cell) fn restore(
        self,
        table: &Restored,
    ) -> io::Result<Box<dyn HistoryCell>> {
        table
            .cell(&self)
            .map(|actual| Box::new(actual) as Box<dyn HistoryCell>)
    }
}
