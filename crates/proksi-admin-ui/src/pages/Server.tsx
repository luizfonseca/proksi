import type { ComponentChildren } from 'preact';
import { useLayoutEffect, useRef, useState } from 'preact/hooks';
import { Badge, Chip } from '../components/ui/Badge';
import { Button } from '../components/ui/Button';
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
    <dl class="grid text-xs">
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

/** A card of settings, all of the same height: taller ones can be expanded */
function Section({ title, children }: { title: string; children: ComponentChildren }) {
  const body = useRef<HTMLDivElement>(null);
  const [overflows, setOverflows] = useState(false);
  const [expanded, setExpanded] = useState(false);
  useLayoutEffect(() => {
    const element = body.current;
    if (element) setOverflows(element.scrollHeight > element.clientHeight + 1);
  }, [children]);
  return (
    <Panel class="flex flex-col px-4 py-3.5">
      <h2 class="mb-2 text-sm font-semibold">{title}</h2>
      <div
        class={`relative overflow-hidden ${expanded ? 'h-auto' : 'h-[196px]'} ${
          overflows && !expanded
            ? "after:pointer-events-none after:absolute after:inset-x-0 after:bottom-0 after:h-10 after:bg-linear-to-b after:from-transparent after:to-base-100 after:content-['']"
            : ''
        }`}
        ref={body}
      >
        {children}
      </div>
      {/* Kept when there's nothing more, so every card has the same height */}
      <div class="mt-2 h-5">
        {(overflows || expanded) && (
          <Button variant="link" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
            {expanded ? 'Show less' : 'Show all'}
          </Button>
        )}
      </div>
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
            <div class="grid grid-cols-[repeat(auto-fill,minmax(340px,1fr))] items-start gap-3 max-sm:grid-cols-1">
              <Section title="General">
                <Rows settings={general} />
              </Section>
              {sections.map(([name, section]) => (
                <Section title={label(name)}>
                  <Rows settings={section} />
                </Section>
              ))}
              {settings.admin === null && (
                <Section title="Admin">
                  <None>off</None>
                </Section>
              )}
            </div>
          );
        }}
      </Loaded>
    </>
  );
}
