import { createStyles } from '@svelteuidev/core';

export default createStyles((theme, params: Record<string, any>) => {
	return {
		root: {
			position: 'absolute',
			left: 0,
			right: 0,
			bottom: 0
		},
		promptWrap: {
			[`${theme.dark} &`]: {
				backgroundColor: theme.colors['dark700'].value
			},
			position: 'relative',
			display: 'flex',
			alignItems: 'flex-end',
			gap: '8px',
			maxWidth: '768px',
			margin: '0 auto',
			padding: '16px 16px 24px',
			backgroundColor: '#fff',
			boxSizing: 'border-box'
		},
		promptInput: {
			[`${theme.dark} &`]: {
				border: '1px solid $dark500',
				backgroundColor: '$dark800',
				color: '$dark50',

				'&:focus, &:focus-within': {
					borderColor: '$blue800'
				}
			},
			display: 'block',
			flex: 1,
			padding: '0 8px',
			backgroundColor: 'White',
			border: '1px solid $gray400',
			borderRadius: '$sm',
			fontSize: '14px',
			lineHeight: '34px',
			outline: 'none',
			resize: 'none',
			transition: 'border-color 100ms ease',

			'&:focus, &:focus-within': {
				borderColor: '$blue500'
			}
		},
		promptAction: {
			position: 'absolute',
			top: '-30px',
			left: '50%',
			transform: 'translateX(-50%)'
		}
	};
});
