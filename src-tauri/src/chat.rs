use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub json: String,
}

pub struct ChatList {
    pub conn: Connection,
}

impl ChatList {
    pub fn new() -> Result<ChatList> {
        let db_path = "db.sqlite";

        let conn = Connection::open(db_path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS Chat (
                id          varchar(64)     PRIMARY KEY,
                json        text            NOT NULL,
                is_delete   numeric         DEFAULT 0
            )",
            [],
        )?;

        Ok(ChatList { conn })
    }

    pub fn get_chats(&self) -> Result<Vec<Chat>> {
        let mut stmt = self.conn.prepare("SELECT * FROM Chat").unwrap();

        let chats_iter = stmt.query_map([], |row| {
            let _is_delete = row.get::<usize, i32>(2).unwrap() == 1;

            Ok(Chat {
                id: row.get(0)?,
                json: row.get(1)?,
            })
        })?;

        let mut chats: Vec<Chat> = Vec::new();

        for chat in chats_iter {
            chats.push(chat?);
        }

        Ok(chats)
    }

    pub fn add_chat(&self, chat: Chat) -> bool {
        let Chat {
            id,
            json,
        } = chat;

        match self
            .conn
            .execute("INSERT INTO Chat (id, json) VALUES (?, ?)", [id, json])
        {
            Ok(insert) => {
                println!("{} row inserted", insert);
                true
            }
            Err(err) => {
                println!("some error: {}", err);
                false
            }
        }
    }

    pub fn del_chat(&self, index: String) -> bool {
        match self
            .conn
            .execute("DELETE FROM Chat WHERE `id` = ?", [index])
        {
            Ok(delete) => {
                println!("{} row deleted", delete);
                true
            }
            Err(err) => {
                println!("some error: {}", err);
                false
            }
        }
    }

    pub fn update(&self, chat: Chat, index: String) -> bool {
        let Chat {
            id: _,
            json,
        } = chat;

        match self
            .conn
            .execute("UPDATE Chat SET `json` = ? WHERE `id` = ?", [json, index])
        {
            Ok(update) => {
                println!("{} row updated", update);
                true
            }
            Err(err) => {
                println!("some error: {}", err);
                false
            }
        }
    }
}
