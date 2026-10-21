use dioxus::prelude::*;

#[derive(Clone, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    #[default]
    Default,
}

#[component]
pub fn Button(
    #[props(into, optional)] class: Option<String>,
    #[props(default = ButtonVariant::default())] variant: ButtonVariant,
    #[props(optional)] disabled: bool,
    #[props(optional)] onclick: Option<EventHandler<MouseEvent>>,
    children: Element,
) -> Element {
    let variant_class = match variant {
        ButtonVariant::Default => "ru-button-default",
    };
    let classes = format!("ru-button {variant_class} {}", class.unwrap_or_default());

    rsx! {
        button {
            class: "{classes}",
            disabled,
            onclick: move |event| {
                if let Some(handler) = &onclick {
                    handler.call(event);
                }
            },
            {children}
        }
    }
}
