use crate::models::{PostView, ReplyView, ReportView, UserView};
#[cfg(feature = "server")]
use dioxus::fullstack::HeaderMap;
use dioxus::prelude::*;

#[cfg(feature = "server")]
mod server_impl {
    use anyhow::{anyhow, bail, Result};
    use argon2::{
        password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    };
    use dioxus::fullstack::{http::header, FullstackContext, HeaderMap, HeaderValue};
    use rydantic::{
        sqlx::{
            query, query_as,
            sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions},
        },
        Model, SqlModel,
    };
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::OnceCell;
    use uuid::Uuid;

    use crate::models::{PostView, ReplyView, ReportView, UserView};

    const CATEGORIES: [&str; 4] = ["Rust 基础", "问题求助", "项目展示", "闲聊"];
    const SESSION_COOKIE: &str = "forum_session";
    const SESSION_TTL: i64 = 7 * 24 * 60 * 60;
    const MAX_IMAGE_BYTES: usize = 512 * 1024;
    static DB: OnceCell<SqlitePool> = OnceCell::const_new();

    #[derive(Debug, Clone, PartialEq, Model, SqlModel)]
    #[sql(table = "posts")]
    struct PostRow {
        #[sql(primary_key, auto_increment)]
        id: i64,
        #[model(trim, min_length = 5, max_length = 120)]
        title: String,
        #[model(trim, min_length = 10, max_length = 10000)]
        body: String,
        #[model(trim, min_length = 1, max_length = 32)]
        author: String,
        #[model(trim, min_length = 1, max_length = 24)]
        #[sql(index)]
        category: String,
        created_at: i64,
        owner_id: Option<i64>,
        #[sql(default = false)]
        is_hidden: bool,
    }

    #[derive(Debug, Clone, PartialEq, Model, SqlModel)]
    #[sql(table = "replies")]
    struct ReplyRow {
        #[sql(primary_key, auto_increment)]
        id: i64,
        #[sql(references = "posts.id", on_delete = "cascade", index)]
        post_id: i64,
        #[model(trim, min_length = 1, max_length = 32)]
        author: String,
        #[model(trim, min_length = 1, max_length = 5000)]
        body: String,
        created_at: i64,
        owner_id: Option<i64>,
        #[sql(default = false)]
        is_hidden: bool,
    }

    pub(super) struct AuthGrant {
        pub(super) user: UserView,
        pub(super) token: String,
    }

    async fn database() -> Result<&'static SqlitePool> {
        Ok(DB
            .get_or_try_init(|| async {
                let options = SqliteConnectOptions::new()
                    .filename("forum.sqlite")
                    .create_if_missing(true)
                    .foreign_keys(true);
                let pool = SqlitePoolOptions::new()
                    .max_connections(8)
                    .connect_with(options)
                    .await?;
                PostRow::migrate(&pool).await?;
                ReplyRow::migrate(&pool).await?;
                for statement in [
                    "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY AUTOINCREMENT, username TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL, role TEXT NOT NULL DEFAULT 'member' CHECK(role IN ('member', 'moderator')), created_at INTEGER NOT NULL)",
                    "CREATE UNIQUE INDEX IF NOT EXISTS users_username_nocase ON users(username COLLATE NOCASE)",
                    "CREATE TABLE IF NOT EXISTS sessions (token_hash TEXT PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE, expires_at INTEGER NOT NULL)",
                    "CREATE INDEX IF NOT EXISTS sessions_expiry_idx ON sessions(expires_at)",
                    "CREATE TABLE IF NOT EXISTS post_images (post_id INTEGER PRIMARY KEY REFERENCES posts(id) ON DELETE CASCADE, content_type TEXT NOT NULL, data BLOB NOT NULL)",
                    "CREATE TABLE IF NOT EXISTS reports (id INTEGER PRIMARY KEY AUTOINCREMENT, target_type TEXT NOT NULL CHECK(target_type IN ('post', 'reply')), target_id INTEGER NOT NULL, reason TEXT NOT NULL, reporter_id INTEGER NOT NULL REFERENCES users(id), status TEXT NOT NULL DEFAULT 'open', created_at INTEGER NOT NULL, UNIQUE(target_type, target_id, reporter_id))",
                    "CREATE TABLE IF NOT EXISTS rate_limits (subject TEXT NOT NULL, action TEXT NOT NULL, window_start INTEGER NOT NULL, count INTEGER NOT NULL, PRIMARY KEY(subject, action))",
                ] {
                    query(statement).execute(&pool).await?;
                }
                query("DELETE FROM sessions WHERE expires_at <= ?")
                    .bind(now())
                    .execute(&pool)
                    .await?;
                query("DELETE FROM rate_limits WHERE window_start < ?")
                    .bind(now() - 24 * 60 * 60)
                    .execute(&pool)
                    .await?;
                Ok::<_, anyhow::Error>(pool)
            })
            .await?)
    }

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    async fn check_rate_limit(
        pool: &SqlitePool,
        subject: &str,
        action: &str,
        limit: i64,
        window_seconds: i64,
    ) -> Result<()> {
        // shortcut: account-scoped limits allow multiple accounts to bypass them; add trusted-proxy IP limits before public deployment.
        let window_start = now() / window_seconds * window_seconds;
        query("INSERT INTO rate_limits (subject, action, window_start, count) VALUES (?, ?, ?, 1) ON CONFLICT(subject, action) DO UPDATE SET count = CASE WHEN rate_limits.window_start = excluded.window_start THEN rate_limits.count + 1 ELSE 1 END, window_start = excluded.window_start")
            .bind(subject)
            .bind(action)
            .bind(window_start)
            .execute(pool)
            .await?;
        let (count,): (i64,) =
            query_as("SELECT count FROM rate_limits WHERE subject = ? AND action = ?")
                .bind(subject)
                .bind(action)
                .fetch_one(pool)
                .await?;
        if count > limit {
            bail!("操作太频繁，请稍后再试");
        }
        Ok(())
    }

    fn token_hash(token: &str) -> String {
        format!("{:x}", Sha256::digest(token.as_bytes()))
    }

    fn cookie_token(headers: &HeaderMap) -> Option<String> {
        headers
            .get(header::COOKIE)?
            .to_str()
            .ok()?
            .split(';')
            .find_map(|part| {
                let (name, value) = part.trim().split_once('=')?;
                (name == SESSION_COOKIE).then(|| value.to_string())
            })
    }

    pub fn set_cookie(token: Option<&str>) -> Result<()> {
        let mut value = match token {
            Some(token) => format!(
                "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={SESSION_TTL}"
            ),
            None => format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"),
        };
        if std::env::var("FORUM_COOKIE_SECURE").as_deref() == Ok("1") {
            value.push_str("; Secure");
        }
        let context = FullstackContext::current().ok_or_else(|| anyhow!("会话上下文不可用"))?;
        context.add_response_header(header::SET_COOKIE, HeaderValue::from_str(&value)?);
        Ok(())
    }

    async fn new_session(pool: &SqlitePool, user: UserView) -> Result<AuthGrant> {
        let token = Uuid::new_v4().simple().to_string();
        query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?, ?, ?)")
            .bind(token_hash(&token))
            .bind(user.id)
            .bind(now() + SESSION_TTL)
            .execute(pool)
            .await?;
        Ok(AuthGrant { user, token })
    }

    async fn user_from_headers(pool: &SqlitePool, headers: &HeaderMap) -> Result<Option<UserView>> {
        let Some(token) = cookie_token(headers) else {
            return Ok(None);
        };
        let row = query_as::<_, (i64, String, String)>(
            "SELECT users.id, users.username, users.role FROM sessions JOIN users ON users.id = sessions.user_id WHERE sessions.token_hash = ? AND sessions.expires_at > ?",
        )
        .bind(token_hash(&token))
        .bind(now())
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|(id, username, role)| UserView { id, username, role }))
    }

    async fn require_user(pool: &SqlitePool, headers: &HeaderMap) -> Result<UserView> {
        user_from_headers(pool, headers)
            .await?
            .ok_or_else(|| anyhow!("请先登录后继续"))
    }

    async fn require_moderator(pool: &SqlitePool, headers: &HeaderMap) -> Result<UserView> {
        let user = require_user(pool, headers).await?;
        if user.role != "moderator" {
            bail!("需要版主权限");
        }
        Ok(user)
    }

    pub async fn register_impl(
        username: String,
        password: String,
        moderator_invite: String,
    ) -> Result<AuthGrant> {
        let pool = database().await?;
        let username = username.trim().to_string();
        if !(3..=24).contains(&username.len())
            || !username
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            bail!("用户名需为 3–24 位字母、数字、下划线或连字符");
        }
        if password.chars().count() < 12 {
            bail!("密码至少需要 12 个字符");
        }
        check_rate_limit(
            pool,
            &format!("register:{}", username.to_ascii_lowercase()),
            "register",
            5,
            3600,
        )
        .await?;
        let role = if moderator_invite.trim().is_empty() {
            "member"
        } else {
            let expected = std::env::var("FORUM_MODERATOR_INVITE")
                .map_err(|_| anyhow!("论坛尚未配置版主邀请码"))?;
            if moderator_invite != expected {
                bail!("版主邀请码无效");
            }
            "moderator"
        };
        let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes())
            .map_err(|error| anyhow!("密码盐生成失败：{error}"))?;
        let password_hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|error| anyhow!("密码加密失败：{error}"))?
            .to_string();
        let result = query(
            "INSERT INTO users (username, password_hash, role, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&username)
        .bind(password_hash)
        .bind(role)
        .bind(now())
        .execute(pool)
        .await
        .map_err(|error| {
            if error.to_string().contains("UNIQUE constraint failed") {
                anyhow!("这个用户名已被使用")
            } else {
                anyhow!(error)
            }
        })?;
        let user = UserView {
            id: result.last_insert_rowid(),
            username,
            role: role.to_string(),
        };
        new_session(pool, user).await
    }

    pub async fn login_impl(username: String, password: String) -> Result<AuthGrant> {
        let pool = database().await?;
        let username = username.trim().to_string();
        check_rate_limit(
            pool,
            &format!("login:{}", username.to_ascii_lowercase()),
            "login",
            10,
            900,
        )
        .await?;
        let row = query_as::<_, (i64, String, String, String)>(
            "SELECT id, username, password_hash, role FROM users WHERE username = ? COLLATE NOCASE",
        )
        .bind(&username)
        .fetch_optional(pool)
        .await?;
        let Some((id, username, password_hash, role)) = row else {
            bail!("用户名或密码错误");
        };
        let parsed_hash = PasswordHash::new(&password_hash).map_err(|_| anyhow!("账号数据无效"))?;
        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_err()
        {
            bail!("用户名或密码错误");
        }
        new_session(pool, UserView { id, username, role }).await
    }

    pub async fn current_user_impl(headers: HeaderMap) -> Result<Option<UserView>> {
        user_from_headers(database().await?, &headers).await
    }

    pub async fn logout_impl(headers: HeaderMap) -> Result<()> {
        if let Some(token) = cookie_token(&headers) {
            query("DELETE FROM sessions WHERE token_hash = ?")
                .bind(token_hash(&token))
                .execute(database().await?)
                .await?;
        }
        Ok(())
    }

    fn post_view(row: PostRow, has_image: bool) -> PostView {
        PostView {
            id: row.id,
            title: row.title,
            body: row.body,
            author: row.author,
            category: row.category,
            created_at: row.created_at,
            author_id: row.owner_id,
            image_url: has_image.then(|| format!("/uploads/{}", row.id)),
        }
    }

    fn reply_view(row: ReplyRow) -> ReplyView {
        ReplyView {
            id: row.id,
            post_id: row.post_id,
            author: row.author,
            body: row.body,
            created_at: row.created_at,
            author_id: row.owner_id,
        }
    }

    pub async fn list_posts_impl() -> Result<Vec<PostView>> {
        let pool = database().await?;
        let posts = query_as::<_, PostRow>(
            "SELECT id, title, body, author, category, created_at, owner_id, is_hidden FROM posts WHERE is_hidden = 0 ORDER BY id DESC",
        )
        .fetch_all(pool)
        .await?;
        let mut views = Vec::with_capacity(posts.len());
        for post in posts {
            let (has_image,): (bool,) =
                query_as("SELECT EXISTS(SELECT 1 FROM post_images WHERE post_id = ?)")
                    .bind(post.id)
                    .fetch_one(pool)
                    .await?;
            views.push(post_view(post, has_image));
        }
        Ok(views)
    }

    fn image_content_type(bytes: &[u8]) -> Option<&'static str> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some("image/png")
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            Some("image/jpeg")
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Some("image/webp")
        } else {
            None
        }
    }

    pub async fn create_post_impl(
        headers: HeaderMap,
        title: String,
        body: String,
        category: String,
        image: Option<Vec<u8>>,
    ) -> Result<PostView> {
        let pool = database().await?;
        let user = require_user(pool, &headers).await?;
        check_rate_limit(pool, &format!("user:{}", user.id), "post", 5, 3600).await?;
        if !CATEGORIES.contains(&category.as_str()) {
            bail!("请选择有效分类");
        }
        let image_type = match image.as_deref() {
            None => None,
            Some(bytes) if bytes.len() > MAX_IMAGE_BYTES => bail!("图片大小不能超过 512 KB"),
            Some(bytes) => Some(
                image_content_type(bytes)
                    .ok_or_else(|| anyhow!("图片格式仅支持 PNG、JPEG 和 WebP"))?,
            ),
        };
        let input = serde_json::json!({
            "id": 0,
            "title": title,
            "body": body,
            "author": user.username,
            "category": category,
            "created_at": now(),
            "owner_id": user.id,
            "is_hidden": false,
        });
        let row = PostRow::parse_json(&input.to_string())
            .map_err(|error| anyhow!("帖子内容校验失败：{error}"))?;
        let row = row.insert(pool).await?;
        if let (Some(bytes), Some(content_type)) = (image, image_type) {
            query("INSERT INTO post_images (post_id, content_type, data) VALUES (?, ?, ?)")
                .bind(row.id)
                .bind(content_type)
                .bind(bytes)
                .execute(pool)
                .await?;
        }
        let (has_image,): (bool,) =
            query_as("SELECT EXISTS(SELECT 1 FROM post_images WHERE post_id = ?)")
                .bind(row.id)
                .fetch_one(pool)
                .await?;
        Ok(post_view(row, has_image))
    }

    pub async fn list_replies_impl(post_id: i64) -> Result<Vec<ReplyView>> {
        let pool = database().await?;
        let replies = query_as::<_, ReplyRow>(
            "SELECT id, post_id, author, body, created_at, owner_id, is_hidden FROM replies WHERE post_id = ? AND is_hidden = 0 ORDER BY id ASC",
        )
        .bind(post_id)
        .fetch_all(pool)
        .await?;
        let mut views = Vec::with_capacity(replies.len());
        for reply in replies {
            views.push(reply_view(reply));
        }
        Ok(views)
    }

    pub async fn create_reply_impl(
        headers: HeaderMap,
        post_id: i64,
        body: String,
    ) -> Result<ReplyView> {
        let pool = database().await?;
        let user = require_user(pool, &headers).await?;
        check_rate_limit(pool, &format!("user:{}", user.id), "reply", 30, 3600).await?;
        let exists = query_as::<_, (i64,)>("SELECT id FROM posts WHERE id = ? AND is_hidden = 0")
            .bind(post_id)
            .fetch_optional(pool)
            .await?;
        if exists.is_none() {
            bail!("这个话题已关闭或不存在");
        }
        let input = serde_json::json!({
            "id": 0,
            "post_id": post_id,
            "author": user.username,
            "body": body,
            "created_at": now(),
            "owner_id": user.id,
            "is_hidden": false,
        });
        let row = ReplyRow::parse_json(&input.to_string())
            .map_err(|error| anyhow!("回复内容校验失败：{error}"))?
            .insert(pool)
            .await?;
        Ok(reply_view(row))
    }

    pub async fn delete_post_impl(headers: HeaderMap, post_id: i64) -> Result<()> {
        let pool = database().await?;
        let user = require_user(pool, &headers).await?;
        let (owner_id,): (Option<i64>,) = query_as("SELECT owner_id FROM posts WHERE id = ?")
            .bind(post_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| anyhow!("话题不存在"))?;
        if owner_id != Some(user.id) && user.role != "moderator" {
            bail!("只能删除自己的话题");
        }
        query("DELETE FROM posts WHERE id = ?")
            .bind(post_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete_reply_impl(headers: HeaderMap, reply_id: i64) -> Result<()> {
        let pool = database().await?;
        let user = require_user(pool, &headers).await?;
        let (owner_id,): (Option<i64>,) = query_as("SELECT owner_id FROM replies WHERE id = ?")
            .bind(reply_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| anyhow!("回复不存在"))?;
        if owner_id != Some(user.id) && user.role != "moderator" {
            bail!("只能删除自己的回复");
        }
        query("DELETE FROM replies WHERE id = ?")
            .bind(reply_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn report_content_impl(
        headers: HeaderMap,
        target_type: String,
        target_id: i64,
        reason: String,
    ) -> Result<()> {
        let pool = database().await?;
        let user = require_user(pool, &headers).await?;
        check_rate_limit(pool, &format!("user:{}", user.id), "report", 5, 3600).await?;
        let (count,): (i64,) = match target_type.as_str() {
            "post" => {
                query_as("SELECT COUNT(*) FROM posts WHERE id = ?")
                    .bind(target_id)
                    .fetch_one(pool)
                    .await?
            }
            "reply" => {
                query_as("SELECT COUNT(*) FROM replies WHERE id = ?")
                    .bind(target_id)
                    .fetch_one(pool)
                    .await?
            }
            _ => bail!("举报对象无效"),
        };
        if count == 0 {
            bail!("举报对象已不存在");
        }
        let reason = reason.trim();
        if !(3..=240).contains(&reason.chars().count()) {
            bail!("举报理由需为 3–240 个字符");
        }
        query("INSERT INTO reports (target_type, target_id, reason, reporter_id, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(target_type)
            .bind(target_id)
            .bind(reason)
            .bind(user.id)
            .bind(now())
            .execute(pool)
            .await
            .map_err(|error| {
                if error.to_string().contains("UNIQUE constraint failed") {
                    anyhow!("你已经举报过这条内容")
                } else {
                    anyhow!(error)
                }
            })?;
        Ok(())
    }

    pub async fn list_reports_impl(headers: HeaderMap) -> Result<Vec<ReportView>> {
        let pool = database().await?;
        require_moderator(pool, &headers).await?;
        let rows = query_as::<_, (i64, String, i64, String, String, String, i64)>(
            "SELECT reports.id, reports.target_type, reports.target_id, reports.reason, users.username, reports.status, reports.created_at FROM reports JOIN users ON users.id = reports.reporter_id ORDER BY reports.created_at DESC LIMIT 100",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, target_type, target_id, reason, reporter, status, created_at)| ReportView {
                    id,
                    target_type,
                    target_id,
                    reason,
                    reporter,
                    status,
                    created_at,
                },
            )
            .collect())
    }

    pub async fn moderate_report_impl(
        headers: HeaderMap,
        report_id: i64,
        action: String,
    ) -> Result<()> {
        let pool = database().await?;
        require_moderator(pool, &headers).await?;
        let (target_type, target_id): (String, i64) =
            query_as("SELECT target_type, target_id FROM reports WHERE id = ?")
                .bind(report_id)
                .fetch_optional(pool)
                .await?
                .ok_or_else(|| anyhow!("举报记录不存在"))?;
        let hidden_sql = match (target_type.as_str(), action.as_str()) {
            ("post", "hide") => "UPDATE posts SET is_hidden = 1 WHERE id = ?",
            ("post", "restore") => "UPDATE posts SET is_hidden = 0 WHERE id = ?",
            ("reply", "hide") => "UPDATE replies SET is_hidden = 1 WHERE id = ?",
            ("reply", "restore") => "UPDATE replies SET is_hidden = 0 WHERE id = ?",
            (_, "hide" | "restore") => bail!("举报对象无效"),
            _ => "",
        };
        if !hidden_sql.is_empty() {
            query(hidden_sql).bind(target_id).execute(pool).await?;
        }
        match action.as_str() {
            "hide" => {
                query("UPDATE reports SET status = 'reviewed' WHERE id = ?")
                    .bind(report_id)
                    .execute(pool)
                    .await?;
            }
            "restore" => {
                query("UPDATE reports SET status = 'restored' WHERE id = ?")
                    .bind(report_id)
                    .execute(pool)
                    .await?;
            }
            "dismiss" => {
                query("UPDATE reports SET status = 'dismissed' WHERE id = ?")
                    .bind(report_id)
                    .execute(pool)
                    .await?;
            }
            _ => bail!("审核操作无效"),
        }
        Ok(())
    }

    pub async fn load_image_impl(post_id: i64) -> Result<Option<(String, Vec<u8>)>> {
        Ok(
            query_as("SELECT post_images.content_type, post_images.data FROM post_images JOIN posts ON posts.id = post_images.post_id WHERE post_images.post_id = ? AND posts.is_hidden = 0")
                .bind(post_id)
                .fetch_optional(database().await?)
                .await?,
        )
    }
}

#[cfg(feature = "server")]
fn set_session_cookie(token: Option<&str>) -> Result<()> {
    Ok(server_impl::set_cookie(token)?)
}

#[server]
pub async fn register(
    username: String,
    password: String,
    moderator_invite: String,
) -> Result<UserView> {
    let grant = server_impl::register_impl(username, password, moderator_invite).await?;
    set_session_cookie(Some(&grant.token))?;
    Ok(grant.user)
}

#[server]
pub async fn login(username: String, password: String) -> Result<UserView> {
    let grant = server_impl::login_impl(username, password).await?;
    set_session_cookie(Some(&grant.token))?;
    Ok(grant.user)
}

#[server(headers: HeaderMap)]
pub async fn current_user() -> Result<Option<UserView>> {
    Ok(server_impl::current_user_impl(headers).await?)
}

#[server(headers: HeaderMap)]
pub async fn logout() -> Result<()> {
    server_impl::logout_impl(headers).await?;
    set_session_cookie(None)?;
    Ok(())
}

#[server]
pub async fn list_posts() -> Result<Vec<PostView>> {
    Ok(server_impl::list_posts_impl().await?)
}

#[server(headers: HeaderMap)]
pub async fn create_post(
    title: String,
    body: String,
    category: String,
    image: Option<Vec<u8>>,
) -> Result<PostView> {
    Ok(server_impl::create_post_impl(headers, title, body, category, image).await?)
}

#[server]
pub async fn list_replies(post_id: i64) -> Result<Vec<ReplyView>> {
    Ok(server_impl::list_replies_impl(post_id).await?)
}

#[server(headers: HeaderMap)]
pub async fn create_reply(post_id: i64, body: String) -> Result<ReplyView> {
    Ok(server_impl::create_reply_impl(headers, post_id, body).await?)
}

#[server(headers: HeaderMap)]
pub async fn delete_post(post_id: i64) -> Result<()> {
    server_impl::delete_post_impl(headers, post_id).await?;
    Ok(())
}

#[server(headers: HeaderMap)]
pub async fn delete_reply(reply_id: i64) -> Result<()> {
    server_impl::delete_reply_impl(headers, reply_id).await?;
    Ok(())
}

#[server(headers: HeaderMap)]
pub async fn report_content(target_type: String, target_id: i64, reason: String) -> Result<()> {
    server_impl::report_content_impl(headers, target_type, target_id, reason).await?;
    Ok(())
}

#[server(headers: HeaderMap)]
pub async fn list_reports() -> Result<Vec<ReportView>> {
    Ok(server_impl::list_reports_impl(headers).await?)
}

#[server(headers: HeaderMap)]
pub async fn moderate_report(report_id: i64, action: String) -> Result<()> {
    server_impl::moderate_report_impl(headers, report_id, action).await?;
    Ok(())
}

#[cfg(feature = "server")]
pub async fn serve_upload(
    axum::extract::Path(post_id): axum::extract::Path<i64>,
) -> axum::response::Response {
    use axum::{http::header, response::IntoResponse};

    match server_impl::load_image_impl(post_id).await {
        Ok(Some((content_type, bytes))) => {
            let mut response = bytes.into_response();
            if let Ok(value) = axum::http::HeaderValue::from_str(&content_type) {
                response.headers_mut().insert(header::CONTENT_TYPE, value);
            }
            response.headers_mut().insert(
                header::X_CONTENT_TYPE_OPTIONS,
                axum::http::HeaderValue::from_static("nosniff"),
            );
            response
        }
        _ => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}
