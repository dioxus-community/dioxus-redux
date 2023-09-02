#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use dioxus_redux::prelude::*;
use dioxus::prelude::*;

fn main() {
    dioxus_desktop::launch(app);
}

#[derive(Clone)]
struct CoolStore {
    tasks: Vec<String>
}

impl CoolStore {
    fn new() -> Self {
        Self {
            tasks: vec!["Todo A".to_string()]
        }
    }

    pub fn tasks(&self) -> TasksSlice {
        TasksSlice(self.tasks.clone())
    }
}

impl Store for CoolStore { }

#[derive(Clone)]
struct TasksSlice(Vec<String>);

fn app(cx: Scope) -> Element {
    use_init_store(cx, CoolStore::new);
    let tasks_slice = use_slice(cx, CoolStore::tasks);

    let tasks = &tasks_slice.read().0;

    render!(
        for task in tasks {
            p {
                "{task}"
            }
        }
        button { }
    )
}
