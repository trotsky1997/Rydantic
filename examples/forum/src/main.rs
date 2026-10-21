mod backend;
mod models;
mod ui;

use backend::{
    create_post, current_user, list_posts, list_reports, login, logout as logout_user,
    moderate_report, register, report_content,
};
use dioxus::prelude::*;
use models::{PostView, ReportView, UserView};
use ui::button::{Button, ButtonVariant};

const CATEGORIES: [&str; 4] = ["Rust 基础", "问题求助", "项目展示", "闲聊"];

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Feed,
    Compose,
    Detail(i64),
    Auth,
    Moderation,
}

fn main() {
    #[cfg(feature = "server")]
    dioxus::serve(|| async move {
        Ok(dioxus::server::router(App).route(
            "/uploads/{post_id}",
            axum::routing::get(backend::serve_upload),
        ))
    });

    #[cfg(not(feature = "server"))]
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut posts = use_signal(Vec::<PostView>::new);
    let mut user = use_signal(|| None::<UserView>);
    let mut screen = use_signal(|| Screen::Feed);
    let mut selected_category = use_signal(|| None::<String>);
    let mut search = use_signal(String::new);
    let mut newest_first = use_signal(|| true);
    let mut loaded = use_signal(|| false);
    let mut load_error = use_signal(|| None::<String>);

    use_effect(move || {
        spawn(async move {
            match list_posts().await {
                Ok(items) => posts.set(items),
                Err(error) => load_error.set(Some(error.to_string())),
            }
            loaded.set(true);
        });
    });

    use_effect(move || {
        spawn(async move {
            if let Ok(current) = current_user().await {
                user.set(current);
            }
        });
    });

    let current_screen = screen();
    let all_posts = posts();
    let current_user = user();
    let authenticated = current_user.is_some();
    let is_moderator = current_user
        .as_ref()
        .is_some_and(|user| user.role == "moderator");
    let category = selected_category();
    let category_heading = category.clone().unwrap_or_else(|| "所有话题".to_string());
    let query = search().trim().to_lowercase();
    let mut visible_posts = all_posts
        .iter()
        .filter(|post| {
            category
                .as_deref()
                .is_none_or(|value| post.category == value)
        })
        .filter(|post| {
            query.is_empty()
                || post.title.to_lowercase().contains(&query)
                || post.body.to_lowercase().contains(&query)
                || post.category.to_lowercase().contains(&query)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !newest_first() {
        visible_posts.reverse();
    }

    rsx! {
        document::Link { rel: "stylesheet", href: asset!("/assets/main.css") }
        header { class: "site-header",
            div { class: "header-inner",
                button { class: "brand", onclick: move |_| screen.set(Screen::Feed),
                    span { class: "brand-mark", "栈" }
                    span {
                        span { class: "brand-name", "栈间" }
                        span { class: "brand-caption", "RUST COMMUNITY" }
                    }
                }
                label { class: "search-wrap",
                    span { class: "search-icon", "⌕" }
                    input {
                        class: "search-input",
                        placeholder: "搜索话题、问题或标签…",
                        value: "{search}",
                        oninput: move |event| search.set(event.value()),
                        "aria-label": "搜索话题",
                    }
                }
                div { class: "header-spacer" }
                if let Some(current) = current_user.clone() {
                    div { class: "header-user",
                        span { class: "avatar coral", "{current.username.chars().next().unwrap_or('栈')}" }
                        span { "{current.username}" }
                        if current.role == "moderator" {
                            button { class: "text-action", onclick: move |_| screen.set(Screen::Moderation), "版主管理" }
                        }
                        button {
                            class: "text-action",
                            onclick: move |_| {
                                spawn(async move {
                                    if logout_user().await.is_ok() {
                                        user.set(None);
                                        screen.set(Screen::Feed);
                                    }
                                });
                            },
                            "退出"
                        }
                    }
                } else {
                    button { class: "login-action", onclick: move |_| screen.set(Screen::Auth), "登录 / 注册" }
                }
            }
        }

        div { class: "layout",
            aside { class: "sidebar",
                section {
                    div { class: "side-label", "社区导航" }
                    div { class: "nav-list",
                        button {
                            class: if selected_category().is_none() { "nav-item active" } else { "nav-item" },
                            onclick: move |_| selected_category.set(None),
                            span { class: "nav-icon", "◈" }
                            "全部话题"
                            span { class: "nav-count", "{all_posts.len()}" }
                        }
                        for (index, name) in CATEGORIES.iter().enumerate() {
                            {
                                let name = (*name).to_string();
                                let active = selected_category().as_deref() == Some(name.as_str());
                                let icon = ["⌘", "?", "✳", "☕"][index];
                                rsx! {
                                    button {
                                        class: if active { "nav-item active" } else { "nav-item" },
                                        onclick: move |_| selected_category.set(Some(name.clone())),
                                        span { class: "nav-icon", "{icon}" }
                                        "{name}"
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "side-divider" }
                section {
                    div { class: "side-label", "你的空间" }
                    div { class: "nav-list",
                        button { class: "nav-item", onclick: move |_| screen.set(if authenticated { Screen::Compose } else { Screen::Auth }),
                            span { class: "nav-icon", "＋" }
                            if authenticated { "发起一个话题" } else { "登录后发起话题" }
                        }
                    }
                }
                div { class: "side-note",
                    div { class: "side-note-title", "欢迎来到栈间" }
                    div { class: "side-note-copy", "这里聊 Rust、分享项目，也一起把问题说清楚。" }
                }
            }

            main { class: "main-column",
                if current_screen == Screen::Auth {
                    AuthPanel {
                        on_cancel: move |_| screen.set(Screen::Feed),
                        on_authenticated: move |current| {
                            user.set(Some(current));
                            screen.set(Screen::Feed);
                        },
                    }
                } else if current_screen == Screen::Moderation && is_moderator {
                    ModerationPanel { on_back: move |_| screen.set(Screen::Feed) }
                } else if current_screen == Screen::Compose && authenticated {
                    ComposePost {
                        on_cancel: move |_| screen.set(Screen::Feed),
                        on_created: move |post| {
                            posts.with_mut(|items| items.insert(0, post));
                            selected_category.set(None);
                            screen.set(Screen::Feed);
                        },
                    }
                } else if let Screen::Detail(id) = current_screen {
                    if let Some(post) = all_posts.iter().find(|post| post.id == id).cloned() {
                        ThreadDetail {
                            post,
                            user: current_user.clone(),
                            on_back: move |_| screen.set(Screen::Feed),
                            on_auth: move |_| screen.set(Screen::Auth),
                            on_deleted: move |id| {
                                posts.with_mut(|items| items.retain(|post| post.id != id));
                                screen.set(Screen::Feed);
                            },
                        }
                    } else {
                        div { class: "empty-state",
                            div { class: "empty-icon", "↗" }
                            div { class: "empty-title", "找不到这个话题" }
                            button { class: "back-button", onclick: move |_| screen.set(Screen::Feed), "← 返回话题列表" }
                        }
                    }
                } else {
                    div { class: "welcome-banner",
                        div { class: "welcome-eyebrow", "一群认真写 Rust 的人" }
                        h1 { class: "welcome-title", "把问题放在桌面上，一起找到答案。" }
                        p { class: "welcome-copy", "从编译错误到新项目，欢迎来栈间聊聊。" }
                    }
                    div { class: "feed-header",
                        div {
                            h2 { class: "feed-title", "{category_heading}" }
                            div { class: "feed-subtitle", "一个给 Rust 开发者的开放讨论区" }
                        }
                        div { class: "feed-actions",
                            div { class: "sort-tabs",
                                button {
                                    class: if newest_first() { "sort-tab active" } else { "sort-tab" },
                                    onclick: move |_| newest_first.set(true),
                                    "最新"
                                }
                                button {
                                    class: if !newest_first() { "sort-tab active" } else { "sort-tab" },
                                    onclick: move |_| newest_first.set(false),
                                    "最早"
                                }
                            }
                            Button {
                                class: "primary-action",
                                variant: ButtonVariant::Default,
                                onclick: move |_| screen.set(if authenticated { Screen::Compose } else { Screen::Auth }),
                                "＋ 发起讨论"
                            }
                        }
                    }
                    if let Some(message) = load_error() {
                        div { class: "empty-state", "暂时无法读取话题：{message}" }
                    } else if !loaded() {
                        div { class: "loading", "正在加载社区话题…" }
                    } else if visible_posts.is_empty() {
                        div { class: "empty-state",
                            div { class: "empty-icon", "✳" }
                            div { class: "empty-title", "这里还没有话题" }
                            div { class: "empty-copy", "来发起第一场讨论吧。" }
                        }
                    } else {
                        div { class: "topic-list",
                            for post in visible_posts {
                                TopicCard {
                                    post,
                                    on_open: move |id| screen.set(Screen::Detail(id)),
                                }
                            }
                        }
                    }
                }
            }

            aside { class: "right-rail",
                section { class: "rail-card",
                    h3 { class: "rail-title", "社区小约定" }
                    div { class: "guideline", span { class: "guideline-mark", "01" } "先说清楚遇到的问题，再贴上最小复现。" }
                    div { class: "guideline", span { class: "guideline-mark", "02" } "分享代码时记得说明 Rust 版本和运行环境。" }
                    div { class: "guideline", span { class: "guideline-mark", "03" } "尊重不同经验，一起把讨论留在技术上。" }
                }
                section { class: "rail-card",
                    h3 { class: "rail-title", "话题分类" }
                    div { class: "tag-list",
                        for name in CATEGORIES {
                            button { class: "tag-chip", onclick: move |_| selected_category.set(Some(name.to_string())), "{name}" }
                        }
                    }
                }
                section { class: "rail-card",
                    h3 { class: "rail-title", "社区概览" }
                    div { class: "stat-grid",
                        div { class: "stat-cell", div { class: "stat-number", "{all_posts.len()}" }, div { class: "stat-label", "个话题" } }
                        div { class: "stat-cell", div { class: "stat-number", "{CATEGORIES.len()}" }, div { class: "stat-label", "个分类" } }
                    }
                }
            }
        }
    }
}

#[component]
fn TopicCard(post: PostView, on_open: EventHandler<i64>) -> Element {
    let post_id = post.id;
    let initial = post.author.chars().next().unwrap_or('访');
    rsx! {
        article { class: "topic-card",
            div { class: "topic-avatar",
                span { class: "avatar blue", "{initial}" }
            }
            div { class: "topic-main",
                div { class: "topic-meta",
                    span { class: "topic-author", "{post.author}" }
                    span { class: "meta-dot" }
                    span { "话题 #{post.id}" }
                    span { class: "meta-dot" }
                    span { class: "topic-category", "{post.category}" }
                }
                button { class: "topic-title", onclick: move |_| on_open.call(post_id), "{post.title}" }
                p { class: "topic-excerpt", "{post.body}" }
                if let Some(image_url) = post.image_url.clone() {
                    img { class: "topic-image", src: "{image_url}", alt: "话题配图" }
                }
                div { class: "topic-footer",
                    span { class: "topic-stat", "↳ 打开讨论" }
                }
            }
        }
    }
}

#[component]
fn ComposePost(on_cancel: EventHandler<()>, on_created: EventHandler<PostView>) -> Element {
    let mut title = use_signal(String::new);
    let mut body = use_signal(String::new);
    let mut category = use_signal(|| CATEGORIES[0].to_string());
    let mut image = use_signal(|| None::<Vec<u8>>);
    let mut image_name = use_signal(|| None::<String>);
    let mut error = use_signal(|| None::<String>);
    let mut submitting = use_signal(|| false);

    let title_value = title();
    let body_value = body();
    let category_value = category();
    let selected_image = image_name();
    let is_submitting = submitting();

    rsx! {
        button { class: "back-button", onclick: move |_| on_cancel.call(()), "← 返回话题列表" }
        section { class: "compose-card",
            h1 { class: "form-title", "发起一个话题" }
            p { class: "form-copy", "把问题、经验或正在做的项目分享给社区。" }
            div { class: "field",
                label { class: "field-label", "话题标题" }
                input {
                    class: "field-input",
                    maxlength: "120",
                    placeholder: "一句话说清楚你想讨论什么",
                    value: "{title_value}",
                    oninput: move |event| title.set(event.value()),
                }
            }
            div { class: "field",
                label { class: "field-label", "分类" }
                select {
                    class: "field-select",
                    value: "{category_value}",
                    onchange: move |event| category.set(event.value()),
                    for name in CATEGORIES { option { value: "{name}", "{name}" } }
                }
            }
            div { class: "field",
                label { class: "field-label", "正文" }
                textarea {
                    class: "field-textarea",
                    maxlength: "10000",
                    placeholder: "补充背景、尝试过的方案，或者你想听听大家意见的部分…",
                    value: "{body_value}",
                    oninput: move |event| body.set(event.value()),
                }
                div { class: "field-help", "至少 10 个字，最多 10,000 个字符。" }
            }
            div { class: "field",
                label { class: "field-label", "配图（可选）" }
                input {
                    class: "field-input",
                    r#type: "file",
                    accept: "image/png,image/jpeg,image/webp",
                    onchange: move |event| async move {
                        if let Some(file) = event.files().into_iter().next() {
                            if file.size() > 512 * 1024 {
                                error.set(Some("图片大小不能超过 512 KB".to_string()));
                                image.set(None);
                                image_name.set(None);
                            } else {
                                match file.read_bytes().await {
                                    Ok(bytes) => {
                                        image.set(Some(bytes.to_vec()));
                                        image_name.set(Some(file.name()));
                                        error.set(None);
                                    }
                                    Err(reason) => error.set(Some(reason.to_string())),
                                }
                            }
                        }
                    },
                }
                if let Some(name) = selected_image { div { class: "field-help", "已选择：{name}" } }
            }
            if let Some(message) = error() { div { class: "form-error", "{message}" } }
            div { class: "form-footer",
                span { class: "field-help", "使用当前登录账号发布；每小时最多 5 个话题。" }
                Button {
                    class: "primary-action",
                    variant: ButtonVariant::Default,
                    disabled: is_submitting,
                    onclick: move |_| {
                        if submitting() { return; }
                        submitting.set(true);
                        error.set(None);
                        let title = title();
                        let body = body();
                        let category = category();
                        let image = image();
                        spawn(async move {
                            match create_post(title, body, category, image).await {
                                Ok(post) => on_created.call(post),
                                Err(reason) => {
                                    error.set(Some(reason.to_string()));
                                    submitting.set(false);
                                }
                            }
                        });
                    },
                    if is_submitting { "发布中…" } else { "发布话题" }
                }
            }
        }
    }
}

#[component]
fn ThreadDetail(
    post: PostView,
    user: Option<UserView>,
    on_back: EventHandler<()>,
    on_deleted: EventHandler<i64>,
    on_auth: EventHandler<()>,
) -> Element {
    use backend::{create_reply, delete_post, delete_reply, list_replies};
    use models::ReplyView;

    let mut replies = use_signal(Vec::<ReplyView>::new);
    let mut loaded = use_signal(|| false);
    let mut load_error = use_signal(|| None::<String>);
    let mut body = use_signal(String::new);
    let mut report_reason = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut submitting = use_signal(|| false);
    let post_id = post.id;

    use_effect(move || {
        spawn(async move {
            match list_replies(post_id).await {
                Ok(items) => replies.set(items),
                Err(reason) => load_error.set(Some(reason.to_string())),
            }
            loaded.set(true);
        });
    });

    let reply_body = body();
    let reason_value = report_reason();
    let is_submitting = submitting();
    let post_initial = post.author.chars().next().unwrap_or('访');
    let can_delete_post = user.as_ref().is_some_and(|user| {
        user.id == post.author_id.unwrap_or_default() || user.role == "moderator"
    });

    rsx! {
        button { class: "back-button", onclick: move |_| on_back.call(()), "← 返回话题列表" }
        article { class: "detail-card",
            div { class: "topic-meta",
                span { class: "avatar blue", "{post_initial}" }
                span { class: "topic-author", "{post.author}" }
                span { class: "meta-dot" }
                span { class: "topic-category", "{post.category}" }
                span { class: "meta-dot" }
                span { "话题 #{post.id}" }
                if can_delete_post {
                    button {
                        class: "text-action danger-action",
                        onclick: move |_| {
                            spawn(async move {
                                match delete_post(post_id).await {
                                    Ok(()) => on_deleted.call(post_id),
                                    Err(reason) => error.set(Some(reason.to_string())),
                                }
                            });
                        },
                        "删除话题"
                    }
                }
            }
            h1 { class: "detail-title", "{post.title}" }
            p { class: "detail-body", "{post.body}" }
            if let Some(image_url) = post.image_url.clone() {
                img { class: "detail-image", src: "{image_url}", alt: "话题配图" }
            }
            if let Some(message) = error() { div { class: "form-error", "{message}" } }
            div { class: "detail-separator" }
            h2 { class: "reply-heading", "讨论回复（{replies().len()}）" }
            if let Some(message) = load_error() {
                div { class: "reply-empty", "读取回复失败：{message}" }
            } else if !loaded() {
                div { class: "reply-empty", "正在读取回复…" }
            } else if replies().is_empty() {
                div { class: "reply-empty", "还没有回复，欢迎开启这场讨论。" }
            } else {
                for reply in replies() {
                    {
                        let reply_id = reply.id;
                        let can_delete_reply = user.as_ref().is_some_and(|user| {
                            user.id == reply.author_id.unwrap_or_default() || user.role == "moderator"
                        });
                        rsx! {
                            div { class: "reply-item",
                                span { class: "avatar green", "{reply.author.chars().next().unwrap_or('访')}" }
                                div { class: "reply-content",
                                    div { class: "reply-meta",
                                        span { class: "reply-author", "{reply.author}" }
                                        span { "楼层 #{reply.id}" }
                                        if can_delete_reply {
                                            button {
                                                class: "text-action danger-action",
                                                onclick: move |_| {
                                                    spawn(async move {
                                                        match delete_reply(reply_id).await {
                                                            Ok(()) => replies.with_mut(|items| items.retain(|item| item.id != reply_id)),
                                                            Err(reason) => error.set(Some(reason.to_string())),
                                                        }
                                                    });
                                                },
                                                "删除"
                                            }
                                        }
                                        if user.is_some() {
                                            button {
                                                class: "text-action",
                                                onclick: move |_| {
                                                    spawn(async move {
                                                        if let Err(reason) = report_content("reply".to_string(), reply_id, "不适当内容".to_string()).await {
                                                            error.set(Some(reason.to_string()));
                                                        }
                                                    });
                                                },
                                                "举报"
                                            }
                                        }
                                    }
                                    p { class: "reply-body", "{reply.body}" }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "detail-separator" }
            h2 { class: "reply-heading", "参与讨论" }
            if user.is_some() {
                div { class: "field",
                    label { class: "field-label", "回复内容" }
                    textarea {
                        class: "field-textarea small",
                        maxlength: "5000",
                        placeholder: "写下你的想法…",
                        value: "{reply_body}",
                        oninput: move |event| body.set(event.value()),
                    }
                }
                div { class: "form-footer",
                    span { class: "field-help", "每小时最多 30 条回复，请保持友善。" }
                    Button {
                        class: "primary-action",
                        variant: ButtonVariant::Default,
                        disabled: is_submitting,
                        onclick: move |_| {
                            if submitting() { return; }
                            submitting.set(true);
                            error.set(None);
                            let text = body();
                            spawn(async move {
                                match create_reply(post_id, text).await {
                                    Ok(reply) => {
                                        replies.with_mut(|items| items.push(reply));
                                        body.set(String::new());
                                        submitting.set(false);
                                    }
                                    Err(reason) => {
                                        error.set(Some(reason.to_string()));
                                        submitting.set(false);
                                    }
                                }
                            });
                        },
                        if is_submitting { "发送中…" } else { "发送回复" }
                    }
                }
                div { class: "report-form",
                    label { class: "field-label", "举报话题" }
                    input {
                        class: "field-input",
                        maxlength: "240",
                        placeholder: "简单说明原因（3–240 字）",
                        value: "{reason_value}",
                        oninput: move |event| report_reason.set(event.value()),
                    }
                    button {
                        class: "text-action",
                        onclick: move |_| {
                            let reason = report_reason();
                            spawn(async move {
                                match report_content("post".to_string(), post_id, reason).await {
                                    Ok(()) => { error.set(Some("举报已提交给版主".to_string())); report_reason.set(String::new()); }
                                    Err(reason) => error.set(Some(reason.to_string())),
                                }
                            });
                        },
                        "提交举报"
                    }
                }
            } else {
                div { class: "login-prompt",
                    span { "登录后可以回复或举报内容。" }
                    button { class: "login-action", onclick: move |_| on_auth.call(()), "登录 / 注册" }
                }
            }
        }
    }
}

#[component]
fn AuthPanel(on_cancel: EventHandler<()>, on_authenticated: EventHandler<UserView>) -> Element {
    let mut registering = use_signal(|| true);
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut invite = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut submitting = use_signal(|| false);
    let is_registering = registering();
    let is_submitting = submitting();

    rsx! {
        button { class: "back-button", onclick: move |_| on_cancel.call(()), "← 返回社区" }
        section { class: "compose-card auth-card",
            h1 { class: "form-title", if is_registering { "创建账号" } else { "登录栈间" } }
            p { class: "form-copy", "登录后即可发帖、回复和举报内容。" }
            div { class: "field",
                label { class: "field-label", "用户名" }
                input {
                    class: "field-input",
                    maxlength: "24",
                    autocomplete: "username",
                    placeholder: "3–24 位字母、数字、下划线或连字符",
                    value: "{username}",
                    oninput: move |event| username.set(event.value()),
                }
            }
            div { class: "field",
                label { class: "field-label", "密码" }
                input {
                    class: "field-input",
                    r#type: "password",
                    minlength: "12",
                    autocomplete: if is_registering { "new-password" } else { "current-password" },
                    placeholder: "至少 12 个字符",
                    value: "{password}",
                    oninput: move |event| password.set(event.value()),
                }
            }
            if is_registering {
                div { class: "field",
                    label { class: "field-label", "版主邀请码（可选）" }
                    input {
                        class: "field-input",
                        r#type: "password",
                        autocomplete: "off",
                        value: "{invite}",
                        oninput: move |event| invite.set(event.value()),
                    }
                    div { class: "field-help", "管理员需在启动服务时配置 FORUM_MODERATOR_INVITE。" }
                }
            }
            if let Some(message) = error() { div { class: "form-error", "{message}" } }
            Button {
                class: "primary-action auth-submit",
                variant: ButtonVariant::Default,
                disabled: is_submitting,
                onclick: move |_| {
                    if submitting() { return; }
                    submitting.set(true);
                    error.set(None);
                    let username = username();
                    let password = password();
                    let invite = invite();
                    spawn(async move {
                        let result = if is_registering {
                            register(username, password, invite).await
                        } else {
                            login(username, password).await
                        };
                        match result {
                            Ok(user) => on_authenticated.call(user),
                            Err(reason) => {
                                error.set(Some(reason.to_string()));
                                submitting.set(false);
                            }
                        }
                    });
                },
                if is_submitting { "请稍候…" } else if is_registering { "创建账号" } else { "登录" }
            }
            button {
                class: "text-action auth-toggle",
                onclick: move |_| { registering.set(!registering()); error.set(None); },
                if is_registering { "已有账号？去登录" } else { "还没有账号？创建一个" }
            }
        }
    }
}

fn refresh_reports(mut reports: Signal<Vec<ReportView>>, mut error: Signal<Option<String>>) {
    spawn(async move {
        match list_reports().await {
            Ok(items) => reports.set(items),
            Err(reason) => error.set(Some(reason.to_string())),
        }
    });
}

fn apply_report_action(
    report_id: i64,
    action: &'static str,
    reports: Signal<Vec<ReportView>>,
    mut error: Signal<Option<String>>,
) {
    spawn(async move {
        match moderate_report(report_id, action.to_string()).await {
            Ok(()) => refresh_reports(reports, error),
            Err(reason) => error.set(Some(reason.to_string())),
        }
    });
}

#[component]
fn ModerationPanel(on_back: EventHandler<()>) -> Element {
    let reports = use_signal(Vec::<ReportView>::new);
    let error = use_signal(|| None::<String>);

    use_effect(move || refresh_reports(reports, error));

    rsx! {
        button { class: "back-button", onclick: move |_| on_back.call(()), "← 返回社区" }
        section { class: "compose-card moderation-card",
            h1 { class: "form-title", "版主管理" }
            p { class: "form-copy", "查看举报并隐藏违规内容；隐藏内容可从这里恢复。" }
            if let Some(message) = error() { div { class: "form-error", "{message}" } }
            if reports().is_empty() {
                div { class: "reply-empty", "目前没有举报记录。" }
            } else {
                for report in reports() {
                    {
                        let report_id = report.id;
                        let target = if report.target_type == "post" { "话题" } else { "回复" };
                        let status = match report.status.as_str() {
                            "open" => "待处理",
                            "reviewed" => "已隐藏",
                            "restored" => "已恢复",
                            "dismissed" => "已忽略",
                            _ => "已处理",
                        };
                        rsx! {
                            article { class: "report-card",
                                div { class: "report-meta",
                                    span { class: "topic-category", "{target} #{report.target_id}" }
                                    span { class: "report-status", "{status}" }
                                }
                                p { class: "reply-body", "{report.reason}" }
                                div { class: "field-help", "由 {report.reporter} 提交" }
                                div { class: "report-actions",
                                    if report.status == "open" {
                                        button {
                                            class: "text-action danger-action",
                                            onclick: move |_| apply_report_action(report_id, "hide", reports, error),
                                            "隐藏并处理"
                                        }
                                        button {
                                            class: "text-action",
                                            onclick: move |_| apply_report_action(report_id, "dismiss", reports, error),
                                            "忽略"
                                        }
                                    } else if report.status == "reviewed" {
                                        button {
                                            class: "text-action",
                                            onclick: move |_| apply_report_action(report_id, "restore", reports, error),
                                            "恢复内容"
                                        }
                                    } else if report.status == "restored" {
                                        button {
                                            class: "text-action danger-action",
                                            onclick: move |_| apply_report_action(report_id, "hide", reports, error),
                                            "再次隐藏"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
