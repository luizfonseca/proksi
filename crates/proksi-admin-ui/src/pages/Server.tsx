import type { ComponentChildren } from 'preact';
import { Loaded } from '../components/State';
import { isSection, label, type Setting, value } from '../settings';
import { useData } from '../useData';

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

function Section({ title, children }: { title: string; children: ComponentChildren }) {
  return (
    <section class="panel settings-section">
      <h2>{title}</h2>
      {children}
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
