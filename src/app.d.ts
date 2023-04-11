// See https://kit.svelte.dev/docs/types#app
// for information about these interfaces
declare global {
	namespace App {
		// interface Error {}
		// interface Locals {}
		// interface PageData {}
		// interface Platform {}
	}
}

export interface ApiKeyType {
	key: string;
	keyOpened: boolean;
	success: boolean;
}

export interface ChatType {
	chatID: string;
	chatTitle: string;
	documentText: string;
	messages: {
		content: string;
		role: 'assistant' | 'system' | 'user';
		finish?: 'stop';
		model?: string;
		usage?: {
			completion_tokens: number;
			prompt_tokens: number;
			total_tokens: number;
		};
	}[];
	model: string;
	cancel?: AbortController;
	loading?: boolean;
	createdAt?: number;
	updatedAt?: number;
	systemMessage?: string;
}

export interface ChatListType {
	chats: ChatType[];
	index: string;
}

// Create chat completion
export type ChatCompletionsType = {
	messages: ChatType['messages'];
	model: string;
};
