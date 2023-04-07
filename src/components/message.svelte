<script lang="ts">
	import { ActionIcon } from '@svelteuidev/core';
	import DOMPurify from 'dompurify';
	import hljs from 'highlight.js';
	import { marked } from 'marked';

	// Resources
	import { IconChatGPT, IconUser } from '../assets/icons';

	// Styles
	import 'highlight.js/styles/github.css';

	// Types
	import type { ChatType } from '../app.d';

	// Props
	export let message: ChatType['messages'][number];

	marked.setOptions({
		breaks: true,
		highlight: (code, lang) => {
			const language = hljs.getLanguage(lang) ? lang : 'plaintext';

			return hljs.highlight(code, { language }).value;
		},
		langPrefix: 'hljs language-'
	});
</script>

<div class="message">
	<div class="role">
		{#if message.role === 'user'}
			<ActionIcon color="dark" size={36} variant="light">
				<IconUser size={24} />
			</ActionIcon>
		{:else}
			<ActionIcon color="#10a37f" size={36} variant="light">
				<IconChatGPT color="#fff" size={24} />
			</ActionIcon>
		{/if}
	</div>

	<div class="content">
		{#if message.role === 'user'}
			{@html `<pre style="padding: 8px; background-color: #438adf; color: #fff; white-space: break-spaces;">${DOMPurify.sanitize(
				message.content
			)}</pre>`}
		{:else}
			{@html DOMPurify.sanitize(marked.parse(message.content))}
		{/if}
	</div>
</div>

<style>
	.message {
		display: flex;
		gap: 12px;
		margin-bottom: 8px;
		padding: 8px;
		border-radius: var(--svelteui-radii-sm);
	}

	.role {
		flex: none;
	}

	.content {
		flex: 1;
		font-size: 14px;
		line-height: 1.7142857;
		word-break: break-all;
	}
	.content :global(:first-child) {
		margin-top: 0;
	}
	.content :global(:last-child) {
		margin-bottom: 0;
	}
	.content :global(code) {
		white-space: pre;
	}
</style>
