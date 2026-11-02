use anyhow::{Context, Result};
use but_api_macros::but_api;
use gitbutler_user::User;
use tracing::instrument;

/// The signed-in account, without any credential.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: u64,
    pub name: Option<String>,
    pub login: Option<String>,
    pub email: Option<String>,
    pub picture: String,
    pub github_username: Option<String>,
}
but_schemars::register_sdk_type!(UserProfile);

impl From<User> for UserProfile {
    fn from(user: User) -> Self {
        UserProfile {
            id: user.id,
            name: user.name,
            login: user.login,
            email: user.email,
            picture: user.picture,
            github_username: user.github_username,
        }
    }
}

/// The signed-in account, or `None`. Credentials stay in this process.
///
/// Only the stored profile is read: the keychain, which may prompt on macOS, is left to the
/// first call that needs the token.
#[but_api]
#[instrument(err(Debug))]
pub fn get_user_profile_local() -> Result<Option<UserProfile>> {
    Ok(gitbutler_user::get_user()?.map(Into::into))
}

/// Complete a login and persist the account, so the token never leaves this process.
#[but_api]
#[instrument(skip(token), err(Debug))]
pub fn login_and_persist(token: String) -> Result<UserProfile> {
    let value = gitbutler_user::api::fetch_user_by_token(&token)?;
    let user: User = serde_json::from_value(value)?;
    // A missing token deserializes to `None` rather than failing, which would store an
    // account that looks signed in and fails every later call instead of this one.
    user.access_token()
        .context("the login response carried no access token")?;
    gitbutler_user::set_user(&user)?;
    Ok(user.into())
}

#[but_api]
#[instrument(err(Debug))]
pub fn delete_user() -> Result<()> {
    gitbutler_user::delete_user()
}

#[but_api]
#[instrument(err(Debug))]
pub fn get_login_token() -> Result<gitbutler_user::api::LoginToken> {
    gitbutler_user::api::fetch_login_token()
}
