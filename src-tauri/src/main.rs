#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

mod chat;
use chat::{Chat, ChatList};

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_chats,
            add_chat,
            del_chat,
            update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
fn get_chats() -> Vec<Chat> {
    let app = ChatList::new().unwrap();
    let chats = app.get_chats().unwrap();
    app.conn.close().unwrap();
    chats
}

#[tauri::command]
fn add_chat(chat: Chat) -> bool {
    let app = ChatList::new().unwrap();
    let result = app.add_chat(chat);
    app.conn.close().unwrap();
    result
}

#[tauri::command]
fn del_chat(index: String) -> bool {
    let app = ChatList::new().unwrap();
    let result = app.del_chat(index);
    app.conn.close().unwrap();
    result
}

#[tauri::command]
fn update(chat: Chat, index: String) -> bool {
    let app = ChatList::new().unwrap();
    let result = app.update(chat, index);
    app.conn.close().unwrap();
    result
}