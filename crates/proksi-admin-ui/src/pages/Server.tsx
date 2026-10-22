import { Badge, Chip } from '../components/ui/Badge';
import { Mono, None, PageHead, Panel } from '../components/ui/Page';
import { Stack } from '../components/ui/Stack';
import { Loaded } from '../components/ui/State';
import { isSection, label, type Setting, value } from '../features/server/settings';
import { useData } from '../api/useData';

type Settings = { [key: string]: Setting };

/** One setting: a value, a list, or tokens (names and access) */
function Value({ name, setting }: { name: string; setting: Setting }) {
  if (Array.isArray(setting)) {
    if (setting.length === 0) return <None>none</None>;
    return (
      <Stack gap={1} wrap align="stretch" justify="end">
        {setting.map((item) =>
          isSection(item) ? (
            <Chip>
              {String(item.name)}
              {item.access ? (
                <Badge size="xs" tone={item.access === 'write' ? 'primary' : 'neutral'} class="ml-1.5 font-sans">
                  {String(item.access)}
                </Badge>
              ) : null}
            </Chip>
          ) : (
            <Chip>{String(item)}</Chip>
          ),
        )}
      </Stack>
    );
  }
  const text = value(name, setting);
  if (typeof setting === 'boolean') return <Badge tone={setting ? 'success' : 'neutral'}>{text}</Badge>;
  if (setting === null) return <None>{text}</None>;
  return <Mono>{text}</Mono>;
}

function Rows({ settings }: { settings: Settings }) {
  return (
    <dl class="grid text-sm">
      {Object.entries(settings).map(([name, setting]) =>
        isSection(setting) ? (
          <div class="border-t border-base-300 py-1.5 first:border-t-0">
            <dt class="whitespace-nowrap text-base-content/60">{label(name)}</dt>
            <dd class="mt-1 ml-3 border-l-2 border-base-300 pl-2.5">
              <Rows settings={setting} />
            </dd>
          </div>
        ) : (
          <div class="flex items-baseline justify-between gap-4 border-t border-base-300 py-1.5 first:border-t-0">
            <dt class="whitespace-nowrap text-base-content/60">{label(name)}</dt>
            <dd class="min-w-0 text-right [overflow-wrap:anywhere]">
              <Value name={name} setting={setting} />
            </dd>
          </div>
        ),
      )}
    </dl>
  );
}

/** A card of settings, as tall as they are: dimmed, and marked off, when its `enabled` is false */
function Card({ title, settings }: { title: string; settings: Settings }) {
  const off = settings.enabled === false;
  return (
    <Panel class={`mb-3 break-inside-avoid px-4 py-3.5 ${off ? 'opacity-60' : ''}`}>
      <Stack class="mb-2">
        <h2 class="text-sm font-semibold">{title}</h2>
        {off && <Badge>off</Badge>}
      </Stack>
      <Rows settings={settings} />
    </Panel>
  );
}

export function Server() {
  const [data] = useData<Settings>('/server', 60000);
  return (
    <>
      <PageHead title="Server" description="The settings Proksi started with, defaults included. Secrets are never shown." />
      <Loaded data={data}>
        {(settings) => {
          const general = Object.fromEntries(Object.entries(settings).filter(([, setting]) => !isSection(setting)));
          const sections = Object.entries(settings).filter(([, setting]) => isSection(setting)) as [string, Settings][];
          return (
            // Columns that cards fill top to bottom: no gaps under short ones
            <div class="gap-3 md:columns-2 xl:columns-3">
              <Card title="General" settings={general} />
              {sections.map(([name, section]) => (
                <Card title={label(name)} settings={section} />
              ))}
              {settings.admin === null && <Card title="Admin" settings={{ enabled: false }} />}
            </div>
          );
        }}
      </Loaded>
    </>
  );
}
