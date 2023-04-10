import Database from 'tauri-plugin-sql-api';

// Types
interface ChatType {
	id: string;
	json: string;
}

// Constants
const db_path = 'sqlite:chat.db';

export const create = async () => {
	const db = await Database.load(db_path);
	return await db.execute(`
        CREATE TABLE IF NOT EXISTS Chat (
            id          varchar(64)     PRIMARY KEY,
            json        text            NOT NULL,
            is_delete   numeric         DEFAULT 0
        )
    `);
};

export async function get_chats() {
	const db = await Database.load(db_path);
	return (await db.select(`SELECT * FROM Chat`)) as ChatType[];
}

export async function add_chat(chat: ChatType) {
	const db = await Database.load(db_path);
	return db.execute(`INSERT INTO Chat (id, json) VALUES ($1, $2)`, [chat.id, chat.json]);
}

export async function del_chat(index: string) {
	const db = await Database.load(db_path);
	return db.execute(`DELETE FROM Chat WHERE id = $1`, [index]);
}

export async function update_chat(chat: ChatType) {
	const db = await Database.load(db_path);
	return db.execute(`UPDATE Chat SET json = $2 WHERE id = $1`, [chat.id, chat.json]);
}
