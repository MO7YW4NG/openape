use super::client::moodle_api_call;
use crate::moodle_args;
use super::types::{ForumDiscussion, ForumPost, SessionInfo};
use crate::utils::strip_html_tags;
use reqwest::Client;
use serde_json::Value;

/// Get forums by course IDs via WS API.
pub async fn get_forums_api(
    client: &Client,
    session: &SessionInfo,
    course_ids: &[u64],
) -> anyhow::Result<Vec<Value>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let course_ids_json: Vec<Value> = course_ids.iter().map(|id| serde_json::json!(*id)).collect();
    let args = moodle_args!("courseids" => course_ids_json);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_forum_get_forums_by_courses", &args).await?;
    Ok(data.as_array().cloned().unwrap_or_default())
}

/// Get discussions in a forum via WS API.
pub async fn get_forum_discussions_api(
    client: &Client,
    session: &SessionInfo,
    forum_id: u64,
    sort_order: Option<i32>,
    page: Option<i32>,
    per_page: Option<i32>,
    group_id: Option<u64>,
) -> anyhow::Result<Vec<ForumDiscussion>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;

    let mut args = moodle_args!("forumid" => forum_id, "sortorder" => sort_order.unwrap_or(2));
    if let Some(p) = page { args.insert("page".to_string(), serde_json::json!(p)); }
    if let Some(pp) = per_page { args.insert("perpage".to_string(), serde_json::json!(pp)); }
    if let Some(g) = group_id { args.insert("groupid".to_string(), serde_json::json!(g)); }

    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_forum_get_forum_discussions", &args).await?;

    let discussions = data.get("discussions")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(discussions.into_iter().map(|d| {
        ForumDiscussion {
            id: d.get("discussion").and_then(|v| v.as_u64()).unwrap_or(0),
            forum_id: d.get("forum").and_then(|v| v.as_u64()).unwrap_or(0),
            name: strip_html_tags(d.get("name").and_then(|v| v.as_str()).unwrap_or("")),
            first_post_id: d.get("firstpost").and_then(|v| v.as_u64()).unwrap_or(0),
            user_id: d.get("userid").and_then(|v| v.as_u64()).unwrap_or(0),
            user_full_name: d.get("userfullname").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            group_id: d.get("groupid").and_then(|v| v.as_u64()),
            time_due: d.get("timedue").and_then(|v| v.as_i64()),
            time_modified: d.get("timemodified").and_then(|v| v.as_i64()).unwrap_or(0),
            time_start: d.get("timestart").and_then(|v| v.as_i64()),
            time_end: d.get("timeend").and_then(|v| v.as_i64()),
            post_count: d.get("numreplies").and_then(|v| v.as_u64()).map(|n| n as u32),
            unread: d.get("numunread").and_then(|v| v.as_u64()).map(|n| n > 0),
            subject: Some(strip_html_tags(d.get("subject").and_then(|v| v.as_str()).unwrap_or(""))),
            message: d.get("message").and_then(|v| v.as_str()).map(|s| s.to_string()),
            pinned: d.get("pinned").and_then(|v| v.as_bool()),
            locked: d.get("locked").and_then(|v| v.as_bool()),
            starred: d.get("starred").and_then(|v| v.as_bool()),
        }
    }).collect())
}

/// Get posts in a discussion via WS API.
pub async fn get_discussion_posts_api(
    client: &Client,
    session: &SessionInfo,
    discussion_id: u64,
) -> anyhow::Result<Vec<ForumPost>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("discussionid" => discussion_id);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_forum_get_discussion_posts", &args).await?;

    let posts = data.get("posts")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(posts.into_iter().map(|p| {
        ForumPost {
            id: p.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            subject: strip_html_tags(p.get("subject").and_then(|v| v.as_str()).unwrap_or("")),
            author: p.get("author").and_then(|a| a.get("fullname")).and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
            author_id: p.get("author").and_then(|a| a.get("id")).and_then(|v| v.as_u64())
                .or_else(|| p.get("userid").and_then(|v| v.as_u64())),
            created: p.get("timecreated").and_then(|v| v.as_i64()).unwrap_or(0),
            modified: p.get("timemodified").and_then(|v| v.as_i64()).unwrap_or(0),
            message: strip_html_tags(p.get("message").and_then(|v| v.as_str()).unwrap_or("")),
            discussion_id: p.get("discussionid").and_then(|v| v.as_u64()).unwrap_or(0),
            unread: p.get("unread").and_then(|v| v.as_bool()),
        }
    }).collect())
}

/// Add a new discussion to a forum.
pub async fn add_discussion_api(
    client: &Client,
    session: &SessionInfo,
    forum_id: u64,
    subject: &str,
    message: &str,
) -> anyhow::Result<Option<u64>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let message_html = message.replace('\n', "<br>");
    let args = moodle_args!(
        "forumid" => forum_id,
        "subject" => subject,
        "message" => message_html
    );
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_forum_add_discussion", &args).await?;
    Ok(data.get("discussionid").and_then(|v| v.as_u64()))
}

/// Reply to a discussion post.
pub async fn add_discussion_post_api(
    client: &Client,
    session: &SessionInfo,
    post_id: u64,
    subject: &str,
    message: &str,
    inline_attachment_id: Option<u64>,
    attachment_id: Option<u64>,
) -> anyhow::Result<Option<u64>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let message_html = message.replace('\n', "<br>");

    let mut api_options = Vec::new();
    if let Some(id) = inline_attachment_id {
        api_options.push(serde_json::json!({"name": "inlineattachmentsid", "value": id}));
    }
    if let Some(id) = attachment_id {
        api_options.push(serde_json::json!({"name": "attachmentsid", "value": id}));
    }

    let mut args = moodle_args!(
        "postid" => post_id,
        "subject" => subject,
        "message" => message_html,
        "messageformat" => 1
    );
    if !api_options.is_empty() {
        args.insert("options".to_string(), serde_json::json!(api_options));
    }

    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_forum_add_discussion_post", &args).await?;
    Ok(data.get("postid").and_then(|v| v.as_u64()))
}

/// Delete a forum post (or entire discussion if first post).
pub async fn delete_post_api(
    client: &Client,
    session: &SessionInfo,
    post_id: u64,
) -> anyhow::Result<bool> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("postid" => post_id);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_forum_delete_post", &args).await?;
    Ok(data.get("status").and_then(|v| v.as_bool()).unwrap_or(false))
}
