import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'LocalGPT',
  tagline: 'Build explorable 3D worlds with natural language — geometry, materials, lighting, audio, and behaviors. Open source, runs locally.',
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
          title: 'Features',
          items: [
            {
              label: 'Gen',
              to: '/docs/gen',
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
              label: 'LocalGPT.md',
              to: '/docs/localgpt',
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
