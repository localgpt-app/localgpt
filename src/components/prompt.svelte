<script lang="ts">
	import { getHotkeyHandler, useId } from '@svelteuidev/composables';
	import { Button, Space } from '@svelteuidev/core';

	// APIs
	import { chatCompletions } from '../requests';

	// Resources
	import { IconArrowRight } from '../assets/icons';

	// Styles
	import useStyles from './prompt.styles';

	// Store
	import { apiKey, chatList, chat } from '../store';

	// Message
	function request(index: string) {
		chatList.update({ chat: { loading: true }, index });

		const tmpChat = $chat!;

		chatCompletions({
			messages: tmpChat.messages.map((message) => ({
				content: message.content,
				role: message.role
			})),
			model: tmpChat.model || 'gpt-3.5-turbo'
		})
			.then(({ data }: any) => {
				// get title
				if (tmpChat.messages.length === 1) {
					chatCompletions({
						messages: [
							...tmpChat.messages.map((message) => ({
								content: message.content,
								role: message.role
							})),
							data.choices[0].message,
							{
								content:
									'What would be a short and relevant title for this chat? You must strictly answer with only the title, no other text is allowed.',
								role: 'user'
							}
						],
						model: tmpChat.model || 'gpt-3.5-turbo'
					}).then(({ data }: any) => {
						chatList.update({ chat: { chatTitle: data.choices[0].message.content }, index });
					});
				}

				// add message
				chatList.addMsg(
					{
						...data,
						...data.choices[0].message
					},
					index
				);

				chatList.update({ chat: { documentText: data.choices[0].message.content }, index });
			})
			.finally(() => {
				chatList.update({ chat: { loading: false }, index });
			});
	}

	let message = '';
	let textarea: HTMLTextAreaElement;
	function add() {
		if ($apiKey.key === '') {
			return apiKey.update({
				keyOpened: true
			});
		}

		if ($chat?.loading || !message) return;

		const chatID = $chat?.chatID || useId();

		if ($chatList.index === '') {
			chatList.add({
				chatID,
				chatTitle: 'New Chat',
				documentText: message,
				messages: [],
				model: 'gpt-3.5-turbo'
			});
		} else {
			chatList.update({
				chat: {
					documentText: message
				}
			});
		}

		chatList.addMsg(
			{
				content: message,
				role: 'user'
			},
			chatID
		);
		message = '';
		textarea.style.height = 'auto';

		request(chatID);
	}

	function change(e: any) {
		const textarea = e.target;
		textarea.style.height = 'auto';
		textarea.style.height = textarea.scrollHeight + 'px';
	}

	$: ({ classes, cx, getStyles } = useStyles({}, { name: 'Prompt' }));
</script>

<div class={cx('prompt', getStyles())}>
	<div class={classes.promptWrap}>
		<textarea
			class={classes.promptInput}
			placeholder="Your message here..."
			rows="1"
			bind:this={textarea}
			bind:value={message}
			on:input={change}
			on:keydown={getHotkeyHandler([['Enter', add]])}
		/>

		<Button on:click={add}>
			<IconArrowRight size={14} />
			<Space w={8} />
			Send
		</Button>
	</div>
</div>
