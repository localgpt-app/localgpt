import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  tutorialSidebar: [
    'intro',
    {
      type: 'category',
      label: 'Getting Started',
      items: ['installation', 'quick-start', 'openclaw-compatibility', 'upgrade-v0.2'],
    },
    {
      type: 'category',
      label: 'CLI Commands',
      items: ['cli-commands', 'cli-chat', 'cli-ask', 'cli-daemon', 'cli-memory'],
    },
    {
      type: 'category',
      label: 'Core Features',
      items: ['memory-system', 'heartbeat', 'tools', 'skills'],
    },
    {
      type: 'category',
      label: 'LocalGPT Gen',
      items: [
        'gen/index',
        'gen/tools',
        'gen/worldgen',
        'gen/behaviors',
        'gen/audio',
        'gen/world-skills',
        'gen/export',
        'gen/mcp-server',
        'gen/cli-mode',
        'gen/headless',
        'gen/external-services',
      ],
    },
    {
      type: 'category',
      label: 'Messaging Bridges',
      items: ['bridges'],
    },
    {
      type: 'category',
      label: 'Security',
      items: ['sandbox', 'localgpt'],
    },
    {
      type: 'category',
      label: 'Reference',
      items: ['architecture', 'configuration', 'http-api'],
    },
    {
      type: 'category',
      label: 'Ecosystem',
      items: ['ecosystem', 'claw', 'worlds'],
    },
  ],
};

export default sidebars;
