<script lang="ts">
	import { Header, Navbar, SvelteUIProvider, TypographyProvider } from '@svelteuidev/core';

	// Components
	import HeaderContent from '../components/header.svelte';
	import NavbarContent from '../components/navbar.svelte';

	// Navbar
	let hidden = false;
	function toggleHidden() {
		hidden = !hidden;
	}

	// Theme
	let isDark = false;
	function toggleTheme() {
		isDark = !isDark;
	}
</script>

<SvelteUIProvider themeObserver={isDark ? 'dark' : 'light'} withGlobalStyles withNormalizeCSS>
	<TypographyProvider>
		<div class="appshell">
			<Navbar
				class="dark-theme"
				height="100vh"
				hidden={!hidden}
				hiddenBreakpoint="md"
				override={{ padding: 8, backgroundColor: '#1a1b1e' }}
				width={{ base: 320 }}
			>
				<NavbarContent />
			</Navbar>

			<div class="body">
				<Header height={56}>
					<HeaderContent {hidden} {isDark} {toggleHidden} {toggleTheme} />
				</Header>

				<div class="main">
					<slot />
				</div>
			</div>
		</div>
	</TypographyProvider>
</SvelteUIProvider>

<style>
	.appshell {
		display: flex;
		height: 100vh;
	}

	.body {
		flex: 1;
		display: flex;
		flex-direction: column;
	}

	.main {
		position: relative;
		flex: 1 0 0;
		overflow: auto;
	}
</style>
