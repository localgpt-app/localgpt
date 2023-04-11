import axios from 'axios';
import store from 'store2';

// Types
import type { AxiosRequestConfig } from 'axios';
import type { ChatCompletionsType } from '../app.d';

// Axios
axios.defaults.baseURL = 'https://api.openai.com';

axios.interceptors.request.use((config) => {
	if (store.has('chat_api_key')) {
		config.headers.Authorization = 'Bearer ' + store.get('chat_api_key');
	}

	return config;
});

// Create chat completion
export const chatCompletions = (data: ChatCompletionsType, config: AxiosRequestConfig = {}) => {
	return axios.post('/v1/chat/completions', data, config);
};

// Verify api key
export const verifyApiKey = (key: string) => {
	const tmpAxios = axios.create();

	return tmpAxios.post(
		'/v1/chat/completions',
		{
			messages: [{ content: 'hello', role: 'user' }],
			model: 'gpt-3.5-turbo'
		},
		{
			headers: { Authorization: 'Bearer ' + key }
		}
	);
};
