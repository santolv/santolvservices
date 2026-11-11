//! Same-origin in-place revisions for the Super Cerebro host.
use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{ArrayBuffer, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wordcraft_doc::{Document, Pos};
use wordcraft_ui_egui::WordApp;

enum Command {
    Flush { name: String },
    Apply { name: String, bytes: Uint8Array, id: u32 },
}

pub struct Live {
    parent: web_sys::Window,
    origin: String,
    commands: Rc<RefCell<Vec<Command>>>,
    reported: Option<(u64, u64)>,
}

impl Live {
    pub fn connect(ctx: &egui::Context) -> Option<Self> {
        let window = web_sys::window()?;
        let parent = window.parent().ok()??;
        if parent == window {
            return None;
        }
        let origin = window.location().origin().ok()?;
        let commands = Rc::new(RefCell::new(Vec::new()));
        let inbox = commands.clone();
        let allowed_parent = JsValue::from(parent.clone());
        let allowed_origin = origin.clone();
        let ctx = ctx.clone();
        let receive = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            if event.origin() != allowed_origin || !Reflect::get(&event, &"source".into()).is_ok_and(|source| source == allowed_parent) {
                return;
            }
            let data = event.data();
            let get = |key: &str| Reflect::get(&data, &key.into()).ok().unwrap_or(JsValue::UNDEFINED);
            let Some(name) = get("name").as_string().filter(|s| !s.is_empty() && s.len() <= 255) else { return };
            let command = match get("type").as_string().as_deref() {
                Some("wordcraft:flush") => Command::Flush { name },
                Some("wordcraft:apply") => {
                    let Ok(bytes) = get("bytes").dyn_into::<ArrayBuffer>() else { return };
                    let Some(id) = get("id").as_f64() else { return };
                    if bytes.byte_length() > 32 * 1024 * 1024 {
                        return;
                    }
                    Command::Apply { name, bytes: Uint8Array::new(&bytes), id: id as u32 }
                }
                _ => return,
            };
            let mut inbox = inbox.borrow_mut();
            if inbox.len() < 4 {
                inbox.push(command);
                ctx.request_repaint();
            }
        });
        window.add_event_listener_with_callback("message", receive.as_ref().unchecked_ref()).ok()?;
        receive.forget(); // The hosted editor lives for the lifetime of this window.
        Some(Self { parent, origin, commands, reported: None })
    }

    fn post(&self, kind: &str, fields: &[(&str, JsValue)]) {
        let message = js_sys::Object::new();
        let _ = Reflect::set(&message, &"type".into(), &kind.into());
        for (key, value) in fields {
            let _ = Reflect::set(&message, &(*key).into(), value);
        }
        let _ = self.parent.post_message(&message, &self.origin);
    }

    pub fn ready(&self) {
        self.post("wordcraft:live-ready", &[]);
    }

    pub fn notify(&mut self, app: &WordApp) {
        let session = &app.session;
        let revision = (session.document_id(), session.rev());
        if session.dirty && self.reported != Some(revision) {
            self.reported = Some(revision);
            self.post("wordcraft:changed", &[]);
        }
    }

    pub fn update(&self, app: &mut WordApp) {
        let commands = std::mem::take(&mut *self.commands.borrow_mut());
        for command in commands {
            match command {
                Command::Flush { name } => {
                    let session = &app.session;
                    if session.dirty && matches_name(session, &name) {
                        if let Err(error) = app.execute("file.save", serde_json::json!({})) {
                            self.post("wordcraft:live-error", &[("error", error.into())]);
                        }
                    }
                    self.post("wordcraft:flushed", &[]);
                }
                Command::Apply { name, bytes, id } => {
                    let session = &mut app.session;
                    let outcome = if !matches_name(session, &name) {
                        "inactive"
                    } else if session.dirty {
                        "busy"
                    } else {
                        let password = session.password.as_ref().map(|value| value.as_str());
                        match wordcraft_engine::io::open_bytes_in(&name, &bytes.to_vec(), password, &session.system_region) {
                            Ok((doc, _)) => {
                                let anchor = map_position(&session.doc, &doc, &session.sel.anchor);
                                let focus = map_position(&session.doc, &doc, &session.sel.focus);
                                session.doc = doc;
                                session.sel.anchor = anchor;
                                session.sel.focus = focus;
                                // Undo stores whole-document snapshots, which could erase a remote edit.
                                session.reset_history();
                                session.clear_versions();
                                session.touch();
                                session.relayout();
                                session.dirty = false;
                                "applied"
                            }
                            Err(_) => "invalid",
                        }
                    };
                    self.post("wordcraft:applied", &[("id", id.into()), ("outcome", outcome.into())]);
                }
            }
        }
    }
}

fn matches_name(session: &wordcraft_engine::Session, name: &str) -> bool {
    session.path.as_ref().and_then(|path| path.file_name()).is_some_and(|file| file == name)
}

fn map_position(before: &Document, after: &Document, position: &Pos) -> Pos {
    let mut mapped = position.clone();
    if let (Some(a), Some(b)) = (before.para_at(position), after.para_at(position)) {
        mapped.off = a.clamp(mapped.off);
        let prefix: usize = a.text.chars().zip(b.text.chars()).take_while(|(a, b)| a == b).map(|(ch, _)| ch.len_utf8()).sum();
        if mapped.off > prefix {
            let a_end = &a.text[prefix..];
            let b_end = &b.text[prefix..];
            let suffix: usize = a_end.chars().rev().zip(b_end.chars().rev()).take_while(|(a, b)| a == b).map(|(ch, _)| ch.len_utf8()).sum();
            mapped.off = if mapped.off >= a.text.len() - suffix { b.text.len() - (a.text.len() - mapped.off) } else { prefix };
        }
    }
    after.clamp(&mapped)
}
