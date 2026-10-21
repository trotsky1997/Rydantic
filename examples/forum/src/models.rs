use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PostView {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub category: String,
    pub created_at: i64,
    pub author_id: Option<i64>,
    pub image_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplyView {
    pub id: i64,
    pub post_id: i64,
    pub author: String,
    pub body: String,
    pub created_at: i64,
    pub author_id: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserView {
    pub id: i64,
    pub username: String,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReportView {
    pub id: i64,
    pub target_type: String,
    pub target_id: i64,
    pub reason: String,
    pub reporter: String,
    pub status: String,
    pub created_at: i64,
}
