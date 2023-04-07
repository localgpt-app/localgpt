<script lang="ts">
	import {
		ActionIcon,
		Button,
		Center,
		Divider,
		Group,
		Input,
		Modal,
		Space,
		Text,
		ThemeIcon,
		Title
	} from '@svelteuidev/core';
	import highlightWords from 'highlight-words';
	import { onMount } from 'svelte';
	import Database from 'tauri-plugin-sql-api';

	// APIs
	import { verifyApiKey } from '../requests';

	// Resources
	import {
		IconCheck,
		IconGear,
		IconMessage,
		IconPen,
		IconPlus,
		IconTrash,
		IconTriangleExclamation,
		IconXmark
	} from '../assets/icons';

	// Styles
	import useStyles from './navbar.styles';

	// Types
	import type { ChatType } from '../app.d';

	// Store
	import { apiKey, chatList } from '../store';

	// Search
	let search = '';

	function getTitle(chatTitle: ChatType['chatTitle']) {
		return highlightWords({
			text: chatTitle,
			query: search
		});
	}

	function getPreview(messages: ChatType['messages'], documentText: ChatType['documentText']) {
		const message = messages.find(
			(message) => search && message.content.toLowerCase().includes(search)
		);
		let text = message?.content || documentText;

		const index = text.indexOf(search);
		text = text.substring(index - 5);

		return highlightWords({
			text,
			query: search
		});
	}

	// Key
	let key = '';
	let keyEnterOpened = false;

	let requestLoading = false;
	let requestError = false;
	function request() {
		requestLoading = true;
		requestError = false;

		verifyApiKey(key)
			.then((res) => {
				apiKey.update({
					key,
					keyOpened: false,
					success: true
				});

				key = '';
				keyEnterOpened = false;
			})
			.catch((err) => {
				requestError = true;
			})
			.finally(() => {
				requestLoading = false;
			});
	}

	// Setting
	let settingOpened = false;

	let chats: ChatType[] = [];
	$: {
		chats = $chatList.chats.filter((chat) => {
			if (search === '') return true;

			return (
				chat.chatTitle.toLowerCase().includes(search) ||
				chat.messages.some((message) => message.content.toLowerCase().includes(search))
			);
		});
	}

	$: ({ classes, cx } = useStyles({}, { name: 'Navbar' }));

	onMount(async () => {
		const db = await Database.load('sqlite:chat.db');

		const result: any[] = await db.select(`SELECT * FROM Chat`);

		chatList.reset(result.map((chat) => JSON.parse(chat.json)));
	});
</script>

<div class={classes.action}>
	<Group direction="column" spacing={8}>
		<Button
			color="gray"
			fullSize
			size="md"
			on:click={() => {
				chatList.update({
					index: ''
				});
			}}
		>
			<IconPlus size={16} />
			<Space w={8} />
			New Chat
		</Button>

		<Input
			class={classes.search}
			override={{ w: '100%', fontSize: 14 }}
			placeholder="Search chats..."
			variant="filled"
			bind:value={search}
		/>
	</Group>
</div>

<div class={classes.chats}>
	{#if search !== '' && chats.length === 0}
		<Text align="center" override={{ fontSize: 14, lineHeight: '22px' }}>No results</Text>
	{/if}

	{#each chats as chat}
		<!-- svelte-ignore a11y-click-events-have-key-events -->
		<div on:click={() => chatList.update({ index: chat.chatID })}>
			<Group class={cx(classes.chat, { active: $chatList.index === chat.chatID })} spacing={8}>
				<ThemeIcon variant="subtle">
					<IconMessage color="#fff" size={16} />
				</ThemeIcon>

				<Group direction="column" override={{ flex: 1, alignItems: 'flex-start' }} spacing={4}>
					<Title lineClamp={1} order={5} override={{ m: '0px !important' }}>
						{#each getTitle(chat.chatTitle) as chunk}
							{#if chunk.match}
								<mark>{chunk.text}</mark>
							{:else}
								{chunk.text}
							{/if}
						{/each}
					</Title>

					<Text color="dimmed" lineClamp={1} size="xs">
						{#each getPreview(chat.messages, chat.documentText) as chunk}
							{#if chunk.match}
								<mark>{chunk.text}</mark>
							{:else}
								{chunk.text}
							{/if}
						{/each}
					</Text>
				</Group>

				<Group spacing={4}>
					<!-- <ActionIcon size={26} variant="default">
						<IconPen size={12} />
					</ActionIcon> -->

					<ActionIcon
						size={26}
						variant="default"
						on:click={(e) => {
							e.stopPropagation();
							chatList.del(chat.chatID);
						}}
					>
						<IconTrash size={12} />
					</ActionIcon>
				</Group>
			</Group>
		</div>
	{/each}
</div>

<div class={classes.footer}>
	<Center>
		<Group spacing={8}>
			<span class={classes.footerTitle}>OpenAI API Key</span>

			<Button
				compact
				override={{ fs: '$xs' }}
				variant="default"
				on:click={() => {
					apiKey.update({
						keyOpened: true
					});
				}}
			>
				{#if $apiKey.key !== ''}
					{#if $apiKey.success}
						<IconCheck color="green" size={12} />
					{:else}
						<IconXmark color="red" size={12} />
					{/if}
					<Space w={8} />
					sk-...{$apiKey.key.substr(-4)}
				{:else}
					<IconTriangleExclamation color="yellow" size={12} />
					<Space w={8} />
					Enter API Key
				{/if}
			</Button>
		</Group>
	</Center>

	<Divider />

	<Center>
		<Group spacing={8}>
			<Button
				compact
				override={{ fs: '$xs' }}
				variant="default"
				on:click={() => {
					window.open('https://github.com/localgpt-app/localgpt/issues', '_blank');
				}}
			>
				<IconMessage size={12} />
				<Space w={8} />
				Send Feedback
			</Button>

			<ActionIcon
				size={26}
				variant="default"
				on:click={() => {
					settingOpened = true;
				}}
			>
				<IconGear size={12} />
			</ActionIcon>
		</Group>
	</Center>
</div>

<!-- Key -->
<Modal
	opened={$apiKey.keyOpened}
	padding="xl"
	withCloseButton={false}
	on:close={() => {
		apiKey.update({
			keyOpened: false
		});
		keyEnterOpened = false;
	}}
>
	<Group direction="column" spacing={16}>
		<Title order={3} override={{ fontSize: 20, lineHeight: '28px' }}>🔑 Enter OpenAI API Key</Title>

		<Text
			color="blue"
			href="https://platform.openai.com/account/api-keys"
			override={{ fontSize: 12, lineHeight: '20px' }}
			root="a"
			target="_blank"
			variant="link"
		>
			→ Get your API key from OpenAI dashboard.
		</Text>

		<Text override={{ fontSize: 12, lineHeight: '20px' }}>
			Your API Key is stored locally on your computer and never sent anywhere else.
		</Text>

		{#if $apiKey.key !== '' && keyEnterOpened === false}
			<Input
				override={{ w: '100%', fontSize: 14 }}
				readonly
				rightSectionWidth={100}
				value={'sk-*************************' + $apiKey.key.substr(-4)}
			>
				<Text
					color="blue"
					override={{ fontSize: 14, lineHeight: '22px' }}
					slot="rightSection"
					variant="link"
					on:click={() => {
						keyEnterOpened = true;
					}}
				>
					Change Key
				</Text>
			</Input>
		{:else}
			<Input
				override={{ w: '100%', fontSize: 14 }}
				placeholder="sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
				rightSectionWidth={100}
				bind:value={key}
			/>

			{#if requestError}
				<Text color="red" override={{ fontSize: 14, lineHeight: '22px' }}>
					Invalid API key. Please make sure your API key is still working properly.
				</Text>
			{/if}

			{#if $apiKey.key !== '' && key === ''}
				<Button
					color="red"
					on:click={() => {
						apiKey.reset();
					}}
				>
					Remove API Key
				</Button>
			{:else}
				<Button
					loading={requestLoading}
					on:click={() => {
						if (key === '') return;

						request();
					}}
				>
					Save API Key
				</Button>
			{/if}
		{/if}
	</Group>
</Modal>

<!-- Setting -->
<Modal
	opened={settingOpened}
	title="Under development"
	withCloseButton={false}
	on:close={() => {
		settingOpened = false;
	}}
>
	Press escape or click on overlay to close.
</Modal>
