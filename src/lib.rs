use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    marker::PhantomData,
    rc::Rc,
    sync::Arc,
};

use dioxus_core::{Scope, ScopeId};
use dioxus_hooks::{to_owned, RefCell};

pub trait Store {
    type Event;

    fn handle(&mut self, event: Self::Event);
}

#[derive(Clone)]
struct Subscription {
    value_entry: ValueEntry,
    subscriptions: Subscriptions,
    function_id: TypeId,
    scope_id: ScopeId,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut subscriptions = self.subscriptions.borrow_mut();

        let no_more_subscriptions = {
            let function = subscriptions.get_mut(&self.function_id);
            if let Some(function) = function {
                // Unsubscribe this scope
                function.scopes.borrow_mut().remove(&self.scope_id);
                function.scopes.borrow().is_empty()
            } else {
                false
            }
        };

        if no_more_subscriptions {
            // Remove the subscription itself if there are no more subscribers
            subscriptions.remove(&self.function_id);
        }
    }
}

type ValueComparer = Rc<dyn Fn(&Rc<RefCell<Box<dyn Any>>>) -> bool>;

#[derive(Clone)]
struct ValueEntry {
    // Scopes subscribed to this value
    scopes: Rc<RefCell<HashSet<ScopeId>>>,
    // The actual value
    value: Rc<RefCell<Box<dyn Any>>>,
    // A function to compare the cached and new value
    compare: ValueComparer,
}

type Subscriptions = Rc<RefCell<HashMap<TypeId, ValueEntry>>>;

pub struct ReduxStore<S: Store> {
    // Actual provided store
    store: Rc<RefCell<S>>,
    // Dispatch events
    event_dispatcher: async_channel::Sender<S::Event>,
    // Subscribers
    subscriptions: Subscriptions,

    schedule_update_any: Arc<dyn Fn(ScopeId)>,
}

impl<S: Store> ReduxStore<S> {
    pub fn handle(&self, event: S::Event) {
        // Notify the store of the new event
        self.store.borrow_mut().handle(event);

        for (_function, value_entry) in self.subscriptions.borrow().iter() {
            let cached_value = &value_entry.value;
            let is_equal = (value_entry.compare)(cached_value);
            if !is_equal {
                // Because the cached and new values were not the same this marks as dirty all the scopes subscribed to those values
                for scope_id in value_entry.scopes.borrow().iter() {
                    (self.schedule_update_any)(*scope_id)
                }
            }
        }
    }

    fn subscribe<V: 'static>(
        &self,
        scope_id: ScopeId,
        function_id: TypeId,
        value: impl FnOnce() -> V,
        compare: impl FnOnce() -> ValueComparer,
    ) -> Subscription {
        let value_entry = {
            let mut subscriptions = self.subscriptions.borrow_mut();
            subscriptions
                .entry(function_id)
                .and_modify(|entry| {
                    entry.scopes.borrow_mut().insert(scope_id);
                })
                .or_insert_with(|| ValueEntry {
                    scopes: Rc::new(RefCell::new(HashSet::from([scope_id]))),
                    value: Rc::new(RefCell::new(Box::new(value()))),
                    compare: compare(),
                })
                .clone()
        };

        Subscription {
            value_entry,
            subscriptions: self.subscriptions.clone(),
            function_id,
            scope_id,
        }
    }
}

impl<S: Store> Clone for ReduxStore<S> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            event_dispatcher: self.event_dispatcher.clone(),
            subscriptions: self.subscriptions.clone(),
            schedule_update_any: self.schedule_update_any.clone(),
        }
    }
}

pub fn use_init_store<S: Store + 'static>(cx: Scope, create_store: impl FnOnce() -> S) {
    cx.use_hook(|| {
        let (event_tx, event_rx) = async_channel::unbounded::<S::Event>();

        let store = cx.provide_context(ReduxStore {
            store: Rc::new(RefCell::new(create_store())),
            event_dispatcher: event_tx,
            subscriptions: Rc::default(),
            schedule_update_any: cx.schedule_update_any(),
        });

        cx.spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                store.handle(event)
            }
        });
    });
}

pub fn use_slice<
    'a,
    F: Copy + 'static + Fn(&S) -> T,
    S: 'static + 'a + Store,
    T: 'static + Clone + PartialEq,
>(
    cx: Scope,
    slicer: F,
) -> &ReduxSlice<T> {
    let store = cx.consume_context::<ReduxStore<S>>().unwrap();
    let subscribe = cx.use_hook({
        to_owned![store];

        move || {
            let gen_value_getter = {
                to_owned![store];
                move || {
                    let store = &store.store.borrow();
                    slicer(store)
                }
            };

            store.subscribe(cx.scope_id(), TypeId::of::<F>(), gen_value_getter, || {
                to_owned![store];
                Rc::new(move |cached: &Rc<RefCell<Box<dyn Any>>>| {
                    let store = &store.store.borrow();
                    let current = slicer(store);

                    // Compare cached and the new value
                    let is_equal = {
                        let cached = cached.borrow();
                        let cached = cached.downcast_ref::<T>().unwrap();
                        cached == &current
                    };

                    if !is_equal {
                        // Update the cached value with the new one
                        *cached.borrow_mut() = Box::new(current);
                    }
                    is_equal
                })
            })
        }
    });

    cx.use_hook(|| ReduxSlice {
        subscribe: Rc::new(subscribe.clone()),
        _phantom: PhantomData,
    })
}

pub struct ReduxSlice<T> {
    subscribe: Rc<Subscription>,
    _phantom: PhantomData<T>,
}

impl<T: 'static> ReduxSlice<T> {
    pub fn read(&self) -> Rc<RefCell<Box<T>>> {
        let value = self.subscribe.value_entry.value.clone();
        downcast(value)
    }
}

fn downcast<T: Any>(v: Rc<RefCell<Box<dyn Any>>>) -> Rc<RefCell<Box<T>>> {
    let v: *const RefCell<Box<dyn Any>> = Rc::into_raw(v);
    unsafe { Rc::from_raw(v as *const RefCell<Box<T>>) }
}

#[derive(Clone)]
pub struct ReduxDispatcher<S: Store> {
    // Dispatch events
    event_dispatcher: async_channel::Sender<S::Event>,
}

impl<S: Store> ReduxDispatcher<S> {
    pub fn dispatch(&self, event: S::Event) {
        // TODO: Handle errors
        self.event_dispatcher.try_send(event).unwrap();
    }
}

pub fn use_dispatcher<S: 'static + Store>(cx: Scope) -> ReduxDispatcher<S> {
    let store = cx.consume_context::<ReduxStore<S>>().unwrap();
    ReduxDispatcher {
        event_dispatcher: store.event_dispatcher,
    }
}

pub mod prelude {
    pub use crate::*;
}
