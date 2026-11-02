// use gitbutler_user::{credentials, credentials::count as count_secrets};
use std::path::{Path, PathBuf};

use but_secret::secret;
use gitbutler_user::User;
use serial_test::serial;

use crate::secret::{credentials, credentials::count as count_secrets};

/// Validate that secrets previously stored in plain-text are auto-migrated into the secrets store.
/// From there, data-structures for use by the frontend need to be 'enriched' with secrets before sending them,
/// or before using them.
#[test]
#[serial]
fn auto_migration_of_secrets_on_when_getting_and_setting_user() -> anyhow::Result<()> {
    for (name, has_github_token) in [("login-only.v1", false), ("with-github.v1", true)] {
        but_testsupport::isolated_app_data_dir(|| -> anyhow::Result<()> {
            credentials::setup();
            let app_data = but_path::app_data_dir()?;
            std::fs::create_dir_all(&app_data)?;

            assert!(
                gitbutler_user::get_user()?.is_none(),
                "Users are bound to logins, so there is none by default"
            );
            assert_eq!(count_secrets(), 0, "no secret is associated with anything");

            let buf = std::fs::read(user_fixture(name))?;
            let user_json_path = app_data.join("user.json");
            std::fs::write(&user_json_path, &buf)?;

            let user = gitbutler_user::get_user()?.expect("previous v1 user was read");
            let expected_secrets = if has_github_token { 2 } else { 1 };
            assert_eq!(
                count_secrets(),
                expected_secrets,
                "it automatically entered the secrets to the secrets store after getting the existing user"
            );

            let assert_access_token_values = |user: &User| -> anyhow::Result<()> {
                assert_eq!(
                    user.access_token()?.0,
                    "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                    "it can make the access token available"
                );
                if has_github_token {
                    assert_eq!(
                        secret::retrieve("github_access_token", secret::Namespace::BuildKind)?
                            .map(|s| s.0),
                        Some("gho_AAAAAAAAAAAAABBBBBBBBBBBBBBBCCCCCCCC".into()),
                        "the GitHub token was migrated into the secret store"
                    );
                }
                Ok(())
            };
            assert_access_token_values(&user)?;

            let assert_no_secret_in_plain_text = || -> anyhow::Result<()> {
                let buf = std::fs::read(&user_json_path)?;
                let value: serde_json::Value = serde_json::from_slice(&buf)?;
                assert_eq!(
                    value.get("access_token"),
                    None,
                    "access token wasn't written back (right after getting it)"
                );
                assert_eq!(
                    value.get("github_access_token"),
                    None,
                    "access token wasn't written back"
                );
                Ok(())
            };
            assert_no_secret_in_plain_text()?;

            let stored_user = gitbutler_user::get_user()?.expect("stored user can be read");
            assert_access_token_values(&stored_user)?;

            gitbutler_user::delete_user()?;
            assert_eq!(
                count_secrets(),
                0,
                "deletion of a user automatically deletes its secretes"
            );
            assert!(
                !user_json_path.exists(),
                "it deletes the whole file, i.e. all associated user data"
            );

            gitbutler_user::set_user(&user)?;
            assert_eq!(
                count_secrets(),
                expected_secrets,
                "the original migrated user still has its secrets, so setting it persists them again"
            );
            assert_no_secret_in_plain_text()?;

            // forget all passwords
            credentials::setup();
            let user = gitbutler_user::get_user()?
                .expect("user still on disk and passwords are accessed lazily");
            assert!(
                user.access_token().is_err(),
                "this is critical - we have a user without access token, this fails early"
            );
            assert!(
                gitbutler_user::get_user()?.is_some(),
                "Client code needs to handle this case and delete the user, \
            otherwise it's there and errors forever"
            );
            Ok(())
        })?;
    }
    Ok(())
}

fn user_fixture(name: &str) -> PathBuf {
    let fixture = Path::new("tests/fixtures/users").join(name);
    assert!(
        fixture.exists(),
        "BUG: fixture at {fixture:?} ought to exist"
    );
    fixture
}
