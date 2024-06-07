use itertools::Itertools;
use std::{borrow::Cow, collections::HashMap};

use super::provider::OauthUser;

/// Parses an HTTP str (from `http::Uri`) to a Hashmap of query parameters
pub(super) fn from_string_to_query_params(value: &str) -> HashMap<Cow<str>, Cow<str>> {
    value
        .split('&')
        .filter_map(|kv| {
            let mut parts = kv.split('=');
            match (parts.next(), parts.next()) {
                (Some(k), Some(v)) => Some((Cow::Borrowed(k), Cow::Borrowed(v))),
                _ => None,
            }
        })
        .collect()
}

/// Given a resulting user from `Oauth2`, validates if the user is authorized
/// based on the validations provided in the configuration of the plugin
pub(super) fn validate_user_from_provider(
    user: &OauthUser,
    validations: Option<&serde_json::Value>,
) -> bool {
    // Authorize if validations is None or not an array
    let Some(validations_array) = validations.and_then(|v| v.as_array()) else {
        return true;
    };

    for validation in validations_array {
        // each validation is an object containing
        // type (string) and value (array)
        let Some(validation_object) = validation.as_object() else {
            continue;
        };

        let Some(validation_type) = validation_object.get("type").and_then(|v| v.as_str()) else {
            continue;
        };

        let validation_values = match validation_object.get("value").and_then(|v| v.as_array()) {
            Some(values) => values.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>(),
            None => continue, // Skip if no value or value is not array
        };

        tracing::info!(
            "Validating {:?} with {:?}",
            validation_type,
            validation_values
        );

        match validation_type {
            "team_id" => {
                // Check if the user's team is in the list of allowed teams
                return validation_values
                    .iter()
                    .any(|v| user.team_ids.contains(&(**v).to_string()));
            }
            "org_id" => {
                // Check if the user's organization is in the list of allowed organizations
                return validation_values
                    .iter()
                    .any(|v| user.organization_ids.contains(&(**v).to_string()));
            }
            "email" => {
                tracing::info!(
                    "Validating email {:?} and {:?}",
                    user.email,
                    validation_values
                );
                // Check if the user's email is in the list of allowed emails
                return validation_values.iter().contains(&&user.email[..]);
            }
            _ => {}
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_no_validations() {
        let user = OauthUser {
            team_ids: vec![],
            organization_ids: vec![],
            email: Cow::Borrowed("user@example.com"),
            usernames: vec![],
        };

        assert!(validate_user_from_provider(&user, None));
    }

    #[test]
    fn test_invalid_validations() {
        let user = OauthUser {
            team_ids: vec![],
            organization_ids: vec![],
            email: Cow::Borrowed("user@example.com"),
            usernames: vec![],
        };

        let invalid_validations = json!(null);
        assert!(validate_user_from_provider(
            &user,
            Some(&invalid_validations)
        ));

        let invalid_validations = json!("not an array");
        assert!(validate_user_from_provider(
            &user,
            Some(&invalid_validations)
        ));
    }

    #[test]
    fn test_validations_with_team_id() {
        let user = OauthUser {
            team_ids: vec!["team1".to_string()],
            organization_ids: vec![],
            email: Cow::Borrowed("user@example.com"),
            usernames: vec![],
        };

        let validations = json!([
            {
                "type": "team_id",
                "value": ["team1", "team2"]
            }
        ]);

        assert!(validate_user_from_provider(&user, Some(&validations)));
    }

    #[test]
    fn test_validations_with_org_id() {
        let user = OauthUser {
            team_ids: vec![],
            organization_ids: vec!["org1".to_string()],
            email: Cow::Borrowed("user@example.com"),
            usernames: vec![],
        };

        let validations = json!([
            {
                "type": "org_id",
                "value": ["org1", "org2"]
            }
        ]);

        assert!(validate_user_from_provider(&user, Some(&validations)));
    }

    #[test]
    fn test_validations_with_email() {
        let user = OauthUser {
            team_ids: vec![],
            organization_ids: vec![],
            email: Cow::Borrowed("user@example.com"),
            usernames: vec![],
        };

        let validations = json!([
            {
                "type": "email",
                "value": ["user@example.com", "other@example.com"]
            }
        ]);

        assert!(validate_user_from_provider(&user, Some(&validations)));
    }

    #[test]
    fn test_validations_no_match() {
        let user = OauthUser {
            team_ids: vec!["team1".to_string()],
            organization_ids: vec!["org1".to_string()],
            email: Cow::Borrowed("user@example.com"),
            usernames: vec![],
        };

        let validations = json!([
            {
                "type": "team_id",
                "value": ["nonexistentteam"]
            },
            {
                "type": "org_id",
                "value": ["nonexistentorg"]
            },
            {
                "type": "email",
                "value": ["nonexistent@example.com"]
            }
        ]);

        let result = validate_user_from_provider(&user, Some(&validations));

        assert!(!result);
    }
}
