import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'LocalGPT',
  tagline: 'Local AI assistant, dreaming explorable worlds.',
  favicon: 'logo/localgpt-icon.svg',

  url: 'https://localgpt.app',
  baseUrl: '/',

  organizationName: 'localgpt-app',
  projectName: 'localgpt-app',

  onBrokenLinks: 'throw',
  onBrokenMarkdownLinks: 'warn',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/localgpt-app/localgpt/tree/main/website/',
        },
        blog: {
          showReadingTime: true,
          editUrl: 'https://github.com/localgpt-app/localgpt/tree/main/website/',
        },
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themes: [
    [
      '@easyops-cn/docusaurus-search-local',
      {
        hashed: true,
        indexBlog: true,
      },
    ],
  ],

  plugins: [
    [
      '@docusaurus/plugin-client-redirects',
      {
        redirects: [
          {
            to: 'https://github.com/localgpt-app/localgpt-gen-workspace/blob/main/skills/desert-pyramids-ufo/SKILL.md',
            from: '/desert-pyramids-ufo',
          },
          {
            // The policy doc moved when LocalGPT.md was renamed to POLICY.md
            to: '/docs/policy',
            from: '/docs/localgpt',
          },
        ],
      },
    ],
  ],

  themeConfig: {
    colorMode: {
      defaultMode: 'dark',
      disableSwitch: true,
      respectPrefersColorScheme: false,
    },
    image: 'logo/localgpt-logo-dark.svg',
    navbar: {
      title: 'LocalGPT',
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'tutorialSidebar',
          position: 'left',
          label: 'Docs',
        },
        {to: '/blog', label: 'Blog', position: 'left'},
        {to: '/templates', label: 'Templates', position: 'left'},
        {
          type: 'dropdown',
          label: 'Apps',
          position: 'left',
          items: [
            {label: 'LocalGPT Gen', to: '/docs/gen'},
            {label: 'LocalGPT Verse', to: '/docs/verse'},
            {label: 'LocalGPT MD', to: '/docs/md'},
          ],
        },
        {
          href: 'https://www.youtube.com/@localgpt-app',
          position: 'right',
          className: 'header-localgpt-app-link',
          'aria-label': 'YouTube',
        },
        {
          href: 'https://www.youtube.com/@localgpt-gen',
          position: 'right',
          className: 'header-localgpt-gen-link',
          'aria-label': 'YouTube Gen Gallery',
        },
        {
          href: 'https://discord.gg/spKRr6mRyp',
          position: 'right',
          className: 'header-discord-link',
          'aria-label': 'Discord',
        },
        {
          href: 'https://x.com/localgpt',
          position: 'right',
          className: 'header-x-link',
          'aria-label': 'X (Twitter)',
        },
        {
          href: 'https://github.com/localgpt-app/localgpt',
          position: 'right',
          className: 'header-github-link',
          'aria-label': 'GitHub repository',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            {
              label: 'Getting Started',
              to: '/docs/intro',
            },
            {
              label: 'CLI Commands',
              to: '/docs/cli-commands',
            },
            {
              label: 'Configuration',
              to: '/docs/configuration',
            },
            {
              label: 'HTTP API',
              to: '/docs/http-api',
            },
          ],
        },
        {
          // The family strip: every LocalGPT site lists the others in this
          // order, with these one-liners.
          title: 'Family',
          items: [
            {
              label: 'LocalGPT Gen',
              href: 'https://gen.localgpt.app/',
              title: 'Worlds from words.',
            },
            {
              label: 'LocalGPT Verse',
              href: 'https://verse.localgpt.app/',
              title: 'A 3D world for every song.',
            },
            {
              label: 'LocalGPT MD',
              href: 'https://md.localgpt.app/',
              title: 'Walk through your notes as a world.',
            },
            {
              label: 'localgpt.world',
              href: 'https://localgpt.world/',
              title: 'Worlds from every app, in your browser.',
            },
            {
              label: 'localgpt.rs',
              href: 'https://localgpt.rs/',
              title: 'The devlog: building it all in Rust.',
            },
          ],
        },
        {
          title: 'Features',
          items: [
            {
              label: 'Collaborative Sessions',
              to: '/docs/gen/multiplayer',
            },
            {
              label: 'Memory System',
              to: '/docs/memory-system',
            },
            {
              label: 'Heartbeat',
              to: '/docs/heartbeat',
            },
            {
              label: 'Shell Sandbox',
              to: '/docs/sandbox',
            },
            {
              label: 'POLICY.md',
              to: '/docs/policy',
            },
          ],
        },
        {
          title: 'Community',
          items: [
            {
              label: 'GitHub',
              href: 'https://github.com/localgpt-app/localgpt',
            },
            {
              label: 'Discord',
              href: 'https://discord.gg/spKRr6mRyp',
            },
            {
              label: 'X (Twitter)',
              href: 'https://x.com/localgpt',
            },
            {
              label: 'Blog',
              to: '/blog',
            },
          ],
        },
        {
          title: 'Showcase',
          items: [
            {
              label: 'World Skills',
              href: 'https://github.com/localgpt-app/workspace',
            },
            {
              label: 'Proof of Video',
              href: 'https://proofof.video/',
            },
            {
              label: 'Gen Gallery',
              href: 'https://www.youtube.com/@localgpt-gen',
            },
            {
              label: 'LocalGPT',
              href: 'https://www.youtube.com/@localgpt-app',
            },
          ],
        },
      ],
      copyright: `Licensed under Apache 2.0`,
    },
    prism: {
      theme: prismThemes.dracula,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['bash', 'toml', 'rust', 'json'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
