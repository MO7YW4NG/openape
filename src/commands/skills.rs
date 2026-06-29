use anyhow::{Context, Result};
use std::path::PathBuf;

const SKILL_NAME: &str = "openape";
const BUNDLED_SKILL: &str = include_str!("../../skills/openape/SKILL.md");

struct Platform {
    name: &'static str,
    path: PathBuf,
}

fn platforms() -> Vec<(&'static str, Platform)> {
    let home = dirs::home_dir().unwrap_or_default();
    vec![
        (
            "claude",
            Platform {
                name: "Claude Code",
                path: home.join(".claude").join("skills"),
            },
        ),
        (
            "codex",
            Platform {
                name: "Codex CLI",
                path: home.join(".codex").join("skills"),
            },
        ),
        (
            "opencode",
            Platform {
                name: "OpenCode",
                path: home.join(".opencode").join("skills"),
            },
        ),
    ]
}

/// Return the skill bundled at compile time.
async fn read_skill_content() -> Result<String> {
    if BUNDLED_SKILL.trim().is_empty() {
        anyhow::bail!("Bundled skill is empty");
    }
    Ok(BUNDLED_SKILL.to_string())
}

pub async fn run(cmd: &crate::SkillsCommands, cli: &crate::Cli) -> Result<()> {
    use crate::output::format_and_output;

    match cmd {
        crate::SkillsCommands::Install { platform, all } => {
            let mut targets: Vec<Platform> = Vec::new();

            if *all {
                for (_, plat) in platforms() {
                    if plat.path.parent().is_some_and(|p| p.exists()) {
                        targets.push(plat);
                    }
                }
                if targets.is_empty() {
                    eprintln!("No supported agents detected. Supported platforms: claude, codex, opencode");
                    let result = serde_json::json!({
                        "action": "install",
                        "skill": SKILL_NAME,
                        "platforms": [],
                        "installed": false,
                    });
                    format_and_output(&[result], cli.output, None);
                    return Ok(());
                }
            } else if let Some(ref p) = platform {
                let key = p.to_lowercase();
                let found = platforms()
                    .into_iter()
                    .find(|(k, _)| *k == key.as_str())
                    .map(|(_, plat)| plat);
                match found {
                    Some(plat) => targets.push(plat),
                    None => {
                        anyhow::bail!(
                            "Unknown platform: {}. Supported: claude, codex, opencode",
                            p
                        );
                    }
                }
            } else {
                anyhow::bail!(
                    "Specify a platform or use --all. Example: openape skills install claude"
                );
            }

            eprintln!("Fetching {} skill...", SKILL_NAME);
            let content = read_skill_content().await?;

            for plat in &targets {
                let dest_dir = plat.path.join(SKILL_NAME);
                tokio::fs::create_dir_all(&dest_dir)
                    .await
                    .with_context(|| format!("Failed to create {}", dest_dir.display()))?;
                tokio::fs::write(dest_dir.join("SKILL.md"), &content)
                    .await
                    .with_context(|| format!("Failed to write to {}", dest_dir.display()))?;
                eprintln!("  {} installed to {}", SKILL_NAME, plat.name);
            }

            let result = serde_json::json!({
                "action": "install",
                "skill": SKILL_NAME,
                "platforms": targets.iter().map(|p| p.name).collect::<Vec<_>>(),
                "installed": true,
            });
            format_and_output(&[result], cli.output, None);
            Ok(())
        }

        crate::SkillsCommands::Show => {
            let content = read_skill_content().await?;
            let result = serde_json::json!({
                "name": SKILL_NAME,
                "content": content,
            });
            format_and_output(&[result], cli.output, None);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_skill_is_present() {
        assert!(BUNDLED_SKILL.contains("name: openape"));
        assert!(BUNDLED_SKILL.contains("openape <command>"));
    }
}
