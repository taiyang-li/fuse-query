// Copyright 2020-2021 The Datafuse Authors.
//
// SPDX-License-Identifier: Apache-2.0.

use common_datavalues::DataSchemaRef;

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
pub struct RemotePlan {
    pub schema: DataSchemaRef,
    pub fetch_name: String,
    pub fetch_nodes: Vec<String>,
}

impl RemotePlan {
    pub fn schema(&self) -> DataSchemaRef {
        self.schema.clone()
    }
}
