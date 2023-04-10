import { findIndex, remove } from 'lodash';
import { derived, writable } from 'svelte/store';
import Database from 'tauri-plugin-sql-api';

// Types
import type { ChatListType, ChatType } from '../app.d';

// Store
import { add_chat, del_chat, update_chat } from './db';

function createChatList() {
	const { set, subscribe, update } = writable<ChatListType>({
		chats: [],
		index: ''
	});

	return {
		add: (chat: ChatType) =>
			update((chatList) => {
				const createdAt = +new Date();
				const tmpChat = { ...chat, createdAt, updatedAt: createdAt };

				add_chat({
					id: tmpChat.chatID,
					json: JSON.stringify(tmpChat)
				});

				return {
					chats: [tmpChat, ...chatList.chats],
					index: chat.chatID
				};
			}),
		del: (index: string) =>
			update((chatList) => {
				remove(chatList.chats, (chat) => chat.chatID === index);

				del_chat(index);

				return {
					chats: [...chatList.chats],
					index: chatList.index === index ? '' : chatList.index
				};
			}),
		addMsg: (message: ChatType['messages'][number], index: string) => {
			update((chatList) => {
				const tmpChat = chatList.chats.find((chat) => chat.chatID === index);

				if (!tmpChat) return chatList;

				tmpChat.messages.push(message);
				tmpChat.updatedAt = +new Date();

				update_chat({
					id: tmpChat.chatID,
					json: JSON.stringify(tmpChat)
				});

				return { ...chatList };
			});
		},
		delMsg: (index: number) => {
			update((chatList) => {
				const tmpChat = chatList.chats.find((chat) => chat.chatID === chatList.index);

				if (!tmpChat) return chatList;

				tmpChat.messages.splice(index, 1);

				update_chat({
					id: tmpChat.chatID,
					json: JSON.stringify(tmpChat)
				});

				return { ...chatList };
			});
		},
		reset: (chats: ChatType[] = []) =>
			set({
				chats,
				index: ''
			}),
		subscribe,
		update: ({ chat, index }: { chat?: Partial<ChatType>; index?: string }) =>
			update((chatList) => {
				index = index !== undefined ? index : chatList.index;

				if (chat) {
					const chatIndex = findIndex(chatList.chats, ['chatID', index]);

					if (chatIndex !== -1) {
						chatList.chats[chatIndex] = { ...chatList.chats[chatIndex], ...chat };

						update_chat({
							id: chatList.chats[chatIndex].chatID,
							json: JSON.stringify(chatList.chats[chatIndex])
						});
					}
				}

				return { ...chatList, index };
			})
	};
}

export const chatList = createChatList();

export const chat = derived(chatList, ($chatList) => {
	return $chatList.chats.find((chat) => chat.chatID === $chatList.index);
});
