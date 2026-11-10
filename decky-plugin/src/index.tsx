import {
  ButtonItem,
  DropdownItem,
  PanelSection,
  PanelSectionRow,
  SliderField,
  TextField,
  ToggleField,
} from "@decky/ui";
import { callable, definePlugin, toaster } from "@decky/api";
import { useCallback, useEffect, useMemo, useState } from "react";
import { FaTv } from "react-icons/fa";

type Value = string | number | boolean | null;

interface Setting {
  key: string;
  type: "string" | "bool" | "int" | "float";
  group: string;
  label: string;
  help: string;
  value: Value;
  default: Value;
  choices?: string[];
  min?: number;
  max?: number;
  step?: number;
}

interface ConfigDump {
  path?: string;
  settings: Setting[];
  error?: string;
}

interface Status {
  installed: boolean;
  version: string;
  active: boolean;
  state: string;
  enabled: boolean;
  panel: boolean;
}

const getStatus = callable<[], Status>("get_status");
const getConfig = callable<[], ConfigDump>("get_config");
const setConfig = callable<[Record<string, Value>], { ok: boolean; error: string }>("set_config");
const restart = callable<[], { ok: boolean; error: string }>("restart");
const latestVersion = callable<[], { tag: string; error: string }>("latest_version");
const update = callable<[], { ok: boolean; log: string; version: string }>("update");
const logs = callable<[number], string>("logs");

function Row({ children }: { children: React.ReactNode }) {
  return <PanelSectionRow>{children}</PanelSectionRow>;
}

function Field({
  s,
  value,
  onChange,
}: {
  s: Setting;
  value: Value;
  onChange: (v: Value) => void;
}) {
  if (s.type === "bool") {
    return (
      <ToggleField
        label={s.label}
        description={s.help}
        checked={value === true}
        onChange={onChange}
      />
    );
  }
  if (s.choices?.length) {
    return (
      <DropdownItem
        label={s.label}
        description={s.help}
        rgOptions={s.choices.map((c) => ({ data: c, label: c }))}
        selectedOption={value as string}
        onChange={(o) => onChange(o.data)}
      />
    );
  }
  if (s.type !== "string" && s.min !== undefined && s.max !== undefined) {
    return (
      <SliderField
        label={s.label}
        description={s.help}
        value={Number(value)}
        min={s.min}
        max={s.max}
        step={s.step ?? 1}
        showValue
        editableValue
        onChange={(v) => onChange(s.type === "int" ? Math.round(v) : v)}
      />
    );
  }
  return (
    <TextField
      label={s.label}
      description={s.help}
      value={String(value ?? "")}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

function Content() {
  const [status, setStatus] = useState<Status | null>(null);
  const [cfg, setCfg] = useState<ConfigDump | null>(null);
  const [draft, setDraft] = useState<Record<string, Value>>({});
  const [latest, setLatest] = useState("");
  const [busy, setBusy] = useState("");
  const [log, setLog] = useState("");

  const refresh = useCallback(async () => {
    setStatus(await getStatus());
    setCfg(await getConfig());
    setDraft({});
  }, []);

  useEffect(() => {
    refresh();
    latestVersion().then((r) => setLatest(r.tag));
  }, [refresh]);

  const groups = useMemo(() => {
    const m = new Map<string, Setting[]>();
    for (const s of cfg?.settings ?? []) {
      m.set(s.group, [...(m.get(s.group) ?? []), s]);
    }
    return [...m.entries()];
  }, [cfg]);

  const dirty = Object.keys(draft).length > 0;

  const apply = async () => {
    setBusy("apply");
    const r = await setConfig(draft);
    toaster.toast({
      title: "BC-250 Dashboard",
      body: r.ok ? "Réglages appliqués" : `Erreur : ${r.error}`,
    });
    await refresh();
    setBusy("");
  };

  const doRestart = async () => {
    setBusy("restart");
    const r = await restart();
    toaster.toast({
      title: "BC-250 Dashboard",
      body: r.ok ? "Dashboard redémarré" : `Erreur : ${r.error}`,
    });
    await refresh();
    setBusy("");
  };

  const doUpdate = async () => {
    setBusy("update");
    const r = await update();
    setLog(r.log);
    toaster.toast({
      title: "BC-250 Dashboard",
      body: r.ok ? `Version ${r.version}` : "Échec de la mise à jour",
    });
    await refresh();
    setBusy("");
  };

  if (status && !status.installed) {
    return (
      <PanelSection title="Non installé">
        <Row>
          Le programme bc250-dashboard est introuvable. Lancez l'installeur en mode Bureau.
        </Row>
      </PanelSection>
    );
  }

  const upToDate = !latest || latest === status?.version;

  return (
    <>
      <PanelSection title="État">
        <Row>
          Service : {status?.active ? "actif" : status?.state ?? "…"}
          <br />
          Écran USB : {status?.panel ? "détecté" : "non détecté"}
          <br />
          Version : {status?.version || "?"}
        </Row>
        <Row>
          <ButtonItem layout="below" disabled={busy !== ""} onClick={doRestart}>
            {busy === "restart" ? "Redémarrage…" : "Redémarrer le dashboard"}
          </ButtonItem>
        </Row>
      </PanelSection>

      <PanelSection title="Mise à jour">
        <Row>
          {upToDate ? "À jour" : `Nouvelle version disponible : ${latest}`}
        </Row>
        <Row>
          <ButtonItem layout="below" disabled={busy !== ""} onClick={doUpdate}>
            {busy === "update"
              ? "Mise à jour…"
              : upToDate
                ? "Vérifier / réinstaller"
                : `Mettre à jour vers ${latest}`}
          </ButtonItem>
        </Row>
        {log && <Row>{log}</Row>}
      </PanelSection>

      {cfg?.error && (
        <PanelSection title="Erreur">
          <Row>{cfg.error}</Row>
        </PanelSection>
      )}

      {groups.map(([group, items]) => (
        <PanelSection key={group} title={group}>
          {items.map((s) => (
            <Row key={s.key}>
              <Field
                s={s}
                value={s.key in draft ? draft[s.key] : s.value}
                onChange={(v) =>
                  setDraft((d) => {
                    const n = { ...d };
                    if (v === s.value) delete n[s.key];
                    else n[s.key] = v;
                    return n;
                  })
                }
              />
            </Row>
          ))}
        </PanelSection>
      ))}

      <PanelSection>
        <Row>
          <ButtonItem
            layout="below"
            disabled={!dirty || busy !== ""}
            onClick={apply}
          >
            {busy === "apply"
              ? "Application…"
              : dirty
                ? `Appliquer (${Object.keys(draft).length}) et redémarrer`
                : "Aucune modification"}
          </ButtonItem>
        </Row>
        <Row>
          <ButtonItem
            layout="below"
            onClick={async () => setLog(await logs(25))}
          >
            Voir les derniers logs
          </ButtonItem>
        </Row>
      </PanelSection>
    </>
  );
}

// Updates are no longer installed automatically: tell the user once per
// Steam session when a newer release exists (applied from the panel).
async function notifyIfUpdate() {
  try {
    const [st, lt] = await Promise.all([getStatus(), latestVersion()]);
    if (st.installed && lt.tag && lt.tag !== st.version) {
      toaster.toast({
        title: "BC-250 Dashboard",
        body: `Mise à jour disponible : ${lt.tag} (menu ... > BC-250 Dashboard)`,
      });
    }
  } catch {
    /* offline or backend not ready: stay silent */
  }
}

export default definePlugin(() => {
  setTimeout(notifyIfUpdate, 15_000);
  return {
    name: "BC-250 Dashboard",
    titleView: <div>BC-250 Dashboard</div>,
    content: <Content />,
    icon: <FaTv />,
    onDismount() {},
  };
});
