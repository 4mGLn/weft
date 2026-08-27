use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StatusJson {
    pub(crate) uncommitted_changes: Vec<UncommittedChangeJson>,
    pub(crate) stacks: Vec<StackJson>,
    pub(crate) merge_base: CommitJson,
    pub(crate) upstream_state: UpstreamStateJson,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UncommittedChangeJson {
    pub(crate) cli_id: String,
    pub(crate) file_path: String,
    pub(crate) change_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StackJson {
    pub(crate) cli_id: String,
    pub(crate) assigned_changes: Vec<Value>,
    pub(crate) branches: Vec<BranchJson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BranchJson {
    pub(crate) cli_id: String,
    pub(crate) name: String,
    pub(crate) commits: Vec<CommitJson>,
    pub(crate) upstream_commits: Vec<CommitJson>,
    pub(crate) branch_status: String,
    pub(crate) review_id: Option<Value>,
    pub(crate) ci: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommitJson {
    pub(crate) cli_id: String,
    pub(crate) change_id: Option<String>,
    pub(crate) commit_id: String,
    pub(crate) created_at: String,
    pub(crate) message: String,
    pub(crate) author_name: String,
    pub(crate) author_email: String,
    pub(crate) conflicted: Option<bool>,
    pub(crate) review_id: Option<Value>,
    pub(crate) changes: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UpstreamStateJson {
    pub(crate) behind: u64,
    pub(crate) latest_commit: CommitJson,
    pub(crate) last_fetched: Option<String>,
}
