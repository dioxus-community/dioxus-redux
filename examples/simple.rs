#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use dioxus::prelude::*;
use dioxus_redux::prelude::*;

fn main() {
    dioxus_desktop::launch(app);
}

#[derive(Clone)]
struct CoolStore {
    tasks: Vec<String>,
}

impl CoolStore {
    fn new() -> Self {
        Self {
            tasks: vec!["Todo A".to_string()],
        }
    }

    pub fn tasks(&self) -> Vec<String> {
        self.tasks.clone()
    }
}

#[allow(dead_code)]
enum CoolStoreEvent {
    PushTask(String),
    PushTasks(Vec<String>),
}

impl Store for CoolStore {
    type Event = CoolStoreEvent;

    fn handle(&mut self, event: Self::Event) {
        match event {
            CoolStoreEvent::PushTask(task) => self.tasks.push(task),
            CoolStoreEvent::PushTasks(tasks) => self.tasks.extend(tasks),
        }
    }
}

fn app(cx: Scope) -> Element {
    use_init_store(cx, CoolStore::new);
    let tasks_slice = use_slice(cx, CoolStore::tasks);
    let dispatcher = use_dispatcher::<CoolStore>(cx);

    let onclick = move |_| dispatcher.dispatch(CoolStoreEvent::PushTask("Hello World".to_string()));

    render!(
        button {
            onclick: onclick,
            "New Task"
        }
        ul {
            for (i, task) in tasks_slice.read().borrow().iter().enumerate() {
                li {
                    key: "{i}",
                    "{task}"
                }
            }
        }
    )
}
