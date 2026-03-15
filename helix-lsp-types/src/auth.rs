use serde::{Deserialize, Serialize};

#[derive(Debug, Eq, PartialEq, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInParams {}

#[derive(Debug, Eq, PartialEq, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptUserDeviceFlowCommand {
    pub command: String,
    pub title: String,
}

#[derive(Debug, Eq, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", tag = "status")]
pub enum SignInResult {
    AlreadySignedIn,

    #[serde(rename_all = "camelCase")]
    PromptUserDeviceFlow {
        user_code: String,
        verification_uri: String,
        command: PromptUserDeviceFlowCommand,
    },
}

#[derive(Debug, Eq, PartialEq, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignOutParams {}

#[derive(Debug, Eq, PartialEq, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignOutResult {}
