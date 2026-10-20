const examples = {
  json: {
    filename: "parse.rs",
    description:
      "Strict types. Explicit coercion. Normalization at the boundary.",
    code: `use rydantic::Model;

#[derive(Debug, Model)]
struct User {
    #[model(trim, min_length = 2)]
    name: String,
    #[model(coerce, ge = 18)]
    age: u8,
}

let user = User::parse_json(
    r#"{"name":" Alice ","age":"25"}"#,
)?;
assert_eq!(user.name, "Alice");
assert_eq!(user.age, 25);`,
  },
  capnp: {
    filename: "protocol.rs",
    description:
      "Generated schema. Official bindings. Native binary messages. Requires capnp.",
    code: `use rydantic::{CapnpModel, CapnpSchema, Model};

#[rydantic::capnp_models(namespace = "app.users")]
mod models {
    #[derive(rydantic::Model)]
    pub struct User {
        pub name: String,
        pub age: u8,
    }
}

let user = models::User::parse_json(
    r#"{"name":"Alice","age":25}"#,
)?;
let bytes = user.dump_capnp()?;
let decoded = models::User::parse_capnp(&bytes)?;
models::User::write_capnp_schema("users.capnp")?;`,
  },
  http: {
    filename: "http.rs",
    description:
      "Validate incoming requests and outgoing responses. Requires axum.",
    code: `use rydantic::{
    Model,
    axum::{Json, Router, routing::post},
};

#[derive(Model)]
struct User {
    #[model(trim, min_length = 2)]
    name: String,
}

async fn create(Json(user): Json<User>) -> Json<User> {
    Json(user)
}

let app: Router = Router::new()
    .route("/users", post(create));`,
  },
  sql: {
    filename: "database.rs",
    description:
      "The pool selects SQLite, PostgreSQL or MySQL. Enable the matching driver.",
    code: `use rydantic::{Model, SqlModel};

#[derive(Model, SqlModel)]
#[sql(table = "users")]
struct User {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[model(trim, min_length = 2)]
    #[sql(unique)]
    name: String,
}

// Inside an async function with a SQLx pool:
User::migrate(&pool).await?;
let user = User::parse_json(r#"{"name":" Alice "}"#)?;
let stored = user.insert(&pool).await?;
let users = User::select().limit(20)
    .fetch_all(&pool).await?;`,
  },
};

const tabs = [...document.querySelectorAll("[data-example]")];
const panel = document.getElementById("example-panel");
const code = document.getElementById("example-code");
let activeExample = "json";
function selectExample(tab, focus = false) {
  activeExample = tab.dataset.example;
  const example = examples[activeExample];
  tabs.forEach((item) => {
    const selected = item === tab;
    item.setAttribute("aria-selected", String(selected));
    item.tabIndex = selected ? 0 : -1;
  });
  panel.setAttribute("aria-labelledby", tab.id);
  document.getElementById("example-filename").textContent = example.filename;
  code.textContent = example.code;
  document.getElementById("example-description").textContent =
    example.description;
  if (focus) tab.focus();
}
tabs.forEach((tab, index) => {
  tab.addEventListener("click", () => selectExample(tab));
  tab.addEventListener("keydown", (event) => {
    let next;
    if (event.key === "ArrowDown" || event.key === "ArrowRight")
      next = (index + 1) % tabs.length;
    if (event.key === "ArrowUp" || event.key === "ArrowLeft")
      next = (index - 1 + tabs.length) % tabs.length;
    if (event.key === "Home") next = 0;
    if (event.key === "End") next = tabs.length - 1;
    if (next !== undefined) {
      event.preventDefault();
      selectExample(tabs[next], true);
    }
  });
});

async function copyText(text, button) {
  try {
    await navigator.clipboard.writeText(text);
    const label = button.querySelector(".copy-label") ?? button;
    const previous = label.textContent;
    label.textContent = "Copied!";
    document.getElementById("copy-status").textContent = "Copied to clipboard.";
    setTimeout(() => {
      label.textContent = previous;
    }, 2000);
  } catch {
    document.getElementById("copy-status").textContent =
      "Copy unavailable. Select and copy the code directly.";
  }
}
document.querySelectorAll("[data-copy]").forEach((button) => {
  button.addEventListener("click", () => copyText(button.dataset.copy, button));
});
document
  .getElementById("copy-example")
  .addEventListener("click", (event) =>
    copyText(examples[activeExample].code, event.currentTarget),
  );

const menu = document.querySelector(".menu-toggle");
const navigation = document.getElementById("navigation");
function closeMenu() {
  menu.setAttribute("aria-expanded", "false");
  navigation.classList.remove("is-open");
}
menu.addEventListener("click", () => {
  const open = menu.getAttribute("aria-expanded") !== "true";
  menu.setAttribute("aria-expanded", String(open));
  navigation.classList.toggle("is-open", open);
});
navigation
  .querySelectorAll("a")
  .forEach((link) => link.addEventListener("click", closeMenu));
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && menu.getAttribute("aria-expanded") === "true") {
    closeMenu();
    menu.focus();
  }
});
