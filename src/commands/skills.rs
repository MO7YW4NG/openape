use anyhow::{Context, Result};
use std::path::PathBuf;

const SKILL_NAME: &str = "openape";
const GITHUB_RAW_URL: &str =
    "https://raw.githubusercontent.com/mo7yw4ng/openape/refs/heads/main/skills/openape/SKILL.md";

struct Platform {
    name: &'static str,
    path: PathBuf,
}

fn platforms() -> Vec<(&'static str, Platform)> {
    let home = dirs::home_dir().unwrap_or_default();
    vec![
        ("claude", Platform {
            name: "Claude Code",
            path: home.join(".claude").join("skills"),
        }),
        ("codex", Platform {
            name: "Codex CLI",
            path: home.join(".codex").join("skills"),
        }),
        ("opencode", Platform {
            name: "OpenCode",
            path: home.join(".opencode").join("skills"),
        }),
    ]
}

/// Try local project path, then next to the executable, then fallback to GitHub.
async fn read_skill_content() -> Result<String> {
    let candidates = [
        // CWD (dev mode / project root)
        std::env::current_dir().unwrap_or_default().join("skills").join(SKILL_NAME).join("SKILL.md"),
        // Next to the executable (npm installed binary)
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|p| p.join("skills").join(SKILL_NAME).join("SKILL.md")))
            .unwrap_or_default(),
    ];

    for path in &candidates {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            if !content.trim().is_empty() {
                return Ok(content);
            }
        }
    }

    let client = reqwest::Client::builder()
        .user_agent("openape-cli")
        .build()?;
    let resp = client
        .get(GITHUB_RAW_URL)
        .send()
        .await
        .with_context(|| "Failed to fetch skill from GitHub")?;

    if !resp.status().is_success() {
        anyhow::bail!("Failed to fetch skill from GitHub: {}", resp.status());
    }

    resp.text().await.with_context(|| "Failed to read response body")
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
                let found = platforms().into_iter()
                    .find(|(k, _)| *k == key.as_str())
                    .map(|(_, plat)| plat);
                match found {
                    Some(plat) => targets.push(plat),
                    None => {
                        anyhow::bail!("Unknown platform: {}. Supported: claude, codex, opencode", p);
                    }
                }
            } else {
                anyhow::bail!("Specify a platform or use --all. Example: openape skills install claude");
            }

            eprintln!("Fetching {} skill...", SKILL_NAME);
            let content = read_skill_content().await?;

            for plat in &targets {
                let dest_dir = plat.path.join(SKILL_NAME);
                tokio::fs::create_dir_all(&dest_dir).await
                    .with_context(|| format!("Failed to create {}", dest_dir.display()))?;
                tokio::fs::write(dest_dir.join("SKILL.md"), &content).await
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
