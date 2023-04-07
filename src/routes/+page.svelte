<script lang="ts">
	// Components
	import { Group, Loader, Text } from '@svelteuidev/core';
	import Message from '../components/message.svelte';
	import Prompt from '../components/prompt.svelte';

	// Store
	import { chat } from '../store';

	let pageElement: HTMLDivElement;

	$: if ($chat?.messages && pageElement) {
		setTimeout(() => {
			const { clientHeight, scrollHeight, scrollTop } = pageElement;

			if (Math.ceil(scrollTop) < scrollHeight - clientHeight) {
				pageElement.scrollTop = scrollHeight;
			}
		});
	}
</script>

<div class="page" bind:this={pageElement}>
	<div class="page-wrap">
		{#each $chat?.messages || [] as message}
			<Message {message} />
		{/each}

		{#if $chat?.loading}
			<Group override={{ padding: 8 }} spacing={8}>
				<Loader color="gray" size="sm" />

				<Text color="gray">Assistant is typing...</Text>
			</Group>
		{/if}
	</div>
</div>

<Prompt />

<style>
	.page {
		position: absolute;
		top: 0;
		left: 0;
		right: 0;
		bottom: 0;
		padding-bottom: 84px;
		overflow: auto;
	}

	.page-wrap {
		max-width: 768px;
		margin: 0 auto;
		padding: 32px 16px;
		box-sizing: border-box;
	}
</style>
