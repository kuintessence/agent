use std::collections::HashMap;

use crate::model::entity::task::execute_usecase::{Requirements, StdInKind};

#[derive(Debug, Clone)]
pub struct ScriptInfo {
    pub id: String,
    pub is_mpi_before_loader: bool,
    pub parent_id: String,
    pub name: String,
    pub load_software: String,
    pub path: String,
    pub arguments: Vec<String>,
    pub environments: HashMap<String, String>,
    pub std_in: Option<StdInKind>,
    pub requirements: Option<Requirements>,
}
