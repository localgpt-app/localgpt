<script lang="ts">
	import { ActionIcon, Burger, Group, Text, Title, UnstyledButton } from '@svelteuidev/core';

	// Resources
	import { IconMoon, IconSun } from '../assets/icons';

	// Store
	import { chat } from '../store';

	// Props
	export let hidden: boolean;
	export let isDark: boolean;
	export let toggleHidden: () => void;
	export let toggleTheme: () => void;

	$: ({ getInfo, getTitle } = (() => {
		const model = $chat?.model.replace(/^gpt-(.+?)-.+/, 'GPT-$1').toUpperCase() || 'GPT-3.5';
		const messages = $chat?.messages.length;

		return {
			getInfo: () => ($chat?.model ? `${model} · ${messages} messages` : 'Start a new chat'),
			getTitle: () => $chat?.chatTitle || 'New Chat'
		};
	})());
</script>

<Group override={{ height: '100%', px: 20 }} position="apart">
	<Burger
		opened={hidden}
		override={{ d: 'block', '@md': { d: 'none' } }}
		size="sm"
		on:click={toggleHidden}
	/>

	<!-- Used as a placeholder to prevent title flickering -->
	<UnstyledButton override={{ d: 'none', w: 28, '@md': { d: 'block' } }} />

	<!-- Title -->
	<Group direction="column" spacing={2}>
		<Title order={5} override={{ m: '0px !important' }}>{getTitle()}</Title>

		<Text color="dimmed" size="xs">{getInfo()}</Text>
	</Group>

	<ActionIcon size={28} variant="default" on:click={toggleTheme}>
		{#if isDark}
			<IconMoon size={16} />
		{:else}
			<IconSun size={16} />
		{/if}
	</ActionIcon>
</Group>
