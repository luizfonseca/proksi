import type { ComponentChildren } from 'preact';
import { useLayoutEffect, useRef, useState } from 'preact/hooks';
import { Loaded } from '../components/ui/State';
import { isSection, label, type Setting, value } from '../features/server/settings';
import { useData } from '../api/useData';

type Settings = { [key: string]: Setting };

/** One setting: a value, a list, or tokens (names and access) */
function Value({ name, setting }: { name: string; setting: Setting }) {
  if (Array.isArray(setting)) {
    if (setting.length === 0) return <span class="none">none</span>;
    return (
      <span class="chips">
        {setting.map((item) =>
          isSection(item) ? (
            <span class="chip">
              {String(item.name)}
              {item.access ? <span class={`badge ${item.access === 'write' ? 'store' : ''}`}>{String(item.access)}</span> : null}
            </span>
          ) : (
            <span class="chip">{String(item)}</span>
          ),
        )}
      </span>
    );
  }
  const text = value(name, setting);
  if (typeof setting === 'boolean') return <span class={`badge ${setting ? 'ok' : ''}`}>{text}</span>;
  if (setting === null) return <span class="none">{text}</span>;
  return <span class="mono">{text}</span>;
}

function Rows({ settings }: { settings: Settings }) {
  return (
    <dl class="settings">
      {Object.entries(settings).map(([name, setting]) =>
        isSection(setting) ? (
          <div class="nested">
            <dt>{label(name)}</dt>
            <dd>
              <Rows settings={setting} />
            </dd>
          </div>
        ) : (
          <div>
            <dt>{label(name)}</dt>
            <dd>
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
    <section class={`panel settings-section${expanded ? ' expanded' : ''}${overflows ? ' overflows' : ''}`}>
      <h2>{title}</h2>
      <div class="settings-body" ref={body}>
        {children}
      </div>
      {/* Kept when there's nothing more, so every card has the same height */}
      <div class="more">
        {(overflows || expanded) && (
          <button type="button" class="link" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
            {expanded ? 'Show less' : 'Show all'}
          </button>
        )}
      </div>
    </section>
  );
}

export function Server() {
  const [data] = useData<Settings>('/server', 60000);
  return (
    <>
      <div class="page-head">
        <div>
          <h1>Server</h1>
          <p>The settings Proksi started with, defaults included. Secrets are never shown.</p>
        </div>
      </div>
      <Loaded data={data}>
        {(settings) => {
          const general = Object.fromEntries(Object.entries(settings).filter(([, setting]) => !isSection(setting)));
          const sections = Object.entries(settings).filter(([, setting]) => isSection(setting)) as [string, Settings][];
          return (
            <div class="settings-grid">
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
                  <span class="none">off</span>
                </Section>
              )}
            </div>
          );
        }}
      </Loaded>
    </>
  );
}
