import store from 'store2';
import { writable } from 'svelte/store';

// Types
import type { ApiKeyType } from '../app.d';

function createApiKey() {
	const { set, subscribe, update } = writable<ApiKeyType>({
		key: store.get('chat_api_key', ''),
		keyOpened: false,
		success: store.get('chat_api_success', true)
	});

	return {
		reset: () => {
			store.remove('chat_api_key');
			store.remove('chat_api_success');

			set({
				key: '',
				keyOpened: false,
				success: true
			});
		},
		subscribe,
		update: (payload: Partial<ApiKeyType>) =>
			update((apiKey) => {
				apiKey = { ...apiKey, ...payload };

				store.set('chat_api_key', apiKey.key);
				store.set('chat_api_success', apiKey.success);

				return apiKey;
			})
	};
}

export const apiKey = createApiKey();
