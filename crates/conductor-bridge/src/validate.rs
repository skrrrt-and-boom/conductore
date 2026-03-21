use crate::BridgeError;

/// Known Claude model short names accepted by the CLI.
pub const VALID_MODELS: &[&str] = &["opus", "sonnet", "haiku"];

/// Pre-flight check: verify the `claude` CLI is installed and accessible.
pub async fn validate_claude_cli() -> Result<(), BridgeError> {
    match tokio::process::Command::new("claude")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
    {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(BridgeError::CliNotFound),
        // CLI exists but may return non-zero — that's fine, it's installed.
        Err(_) | Ok(_) => Ok(()),
    }
}

/// Validate that `model` is one of the accepted short names.
pub fn validate_model(model: &str) -> Result<(), BridgeError> {
    if VALID_MODELS.contains(&model) {
        Ok(())
    } else {
        Err(BridgeError::UnknownModel(
            model.to_string(),
            VALID_MODELS.join(", "),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_models_accepted() {
        for model in VALID_MODELS {
            assert!(validate_model(model).is_ok(), "expected {model} to be valid");
        }
    }

    #[test]
    fn unknown_model_rejected() {
        let err = validate_model("gpt-4").unwrap_err();
        assert!(matches!(err, BridgeError::UnknownModel(m, _) if m == "gpt-4"));
    }
}
