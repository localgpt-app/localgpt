import { createStyles } from '@svelteuidev/core';

export default createStyles((theme, params: Record<string, any>) => {
	return {
		action: {
			paddingBottom: 8
		},
		search: {
			color: '#fff !important'
		},
		chats: {
			flex: 1,
			color: '#ececf1',
			overflow: 'auto'
		},
		chat: {
			marginBottom: 8,
			padding: 8,
			borderRadius: '$sm',

			'&:hover': {
				backgroundColor: '#2c2e33',
				cursor: 'pointer'
			},
			'&.active': {
				backgroundColor: '#2c2e33'
			}
		},
		footer: {
			margin: '0 -8px -8px',
			padding: '16px 0',
			backgroundColor: '#343540'
		},
		footerTitle: {
			color: '#fff',
			fontSize: 12
		}
	};
});
