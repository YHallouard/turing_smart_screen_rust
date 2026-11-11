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
  usb: boolean;
  udev_rule: boolean;
  can_repair: boolean;
}

const getStatus = callable<[], Status>("get_status");
const getConfig = callable<[], ConfigDump>("get_config");
const setConfig = callable<[Record<string, Value>], { ok: boolean; error: string }>("set_config");
const restart = callable<[], { ok: boolean; error: string }>("restart");
const latestVersion = callable<[], { tag: string; error: string }>("latest_version");
const update = callable<[], { ok: boolean; log: string; version: string }>("update");
const logs = callable<[number], string>("logs");
interface FpsDiag {
  mangoapp: boolean;
  config: string;
  block: boolean;
  csv_age: number | null;
}
const fpsDiag = callable<[], FpsDiag>("fps_diag");
const repairUdev = callable<[], { ok: boolean; error: string }>("repair_udev");

// Logs stay small: a few short lines, never a wall of text.
const LOG_STYLE: React.CSSProperties = {
  fontFamily: "monospace",
  fontSize: 10,
  lineHeight: 1.3,
  whiteSpace: "pre-wrap",
  wordBreak: "break-all",
  maxHeight: 140,
  overflowY: "auto",
  opacity: 0.85,
};

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
  const options = useMemo(
    () => (s.choices ?? []).map((c) => ({ data: c, label: c })),
    [s.choices],
  );
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
        rgOptions={options}
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

// The quick-access menu unmounts the panel when it closes; keep unsaved edits.
let savedDraft: Record<string, Value> = {};

function Content() {
  const [status, setStatus] = useState<Status | null>(null);
  const [cfg, setCfg] = useState<ConfigDump | null>(null);
  const [draft, setDraftState] = useState<Record<string, Value>>(savedDraft);
  const setDraft = (f: (d: Record<string, Value>) => Record<string, Value>) =>
    setDraftState((d) => (savedDraft = f(d)));
  const [latest, setLatest] = useState("");
  const [busy, setBusy] = useState("");
  const [updLog, setUpdLog] = useState("");
  const [logText, setLogText] = useState<string | null>(null);
  const [fps, setFps] = useState<FpsDiag | null>(null);

  const refresh = useCallback(async () => {
    setStatus(await getStatus());
    setCfg(await getConfig());
    setDraft(() => ({}));
  }, []);

  useEffect(() => {
    refresh();
    latestVersion().then((r) => setLatest(r.tag));
    // Keep the state live (service starting, panel plugged in…).
    const t = setInterval(
      () =>
        getStatus().then((st) =>
          setStatus((prev) =>
            JSON.stringify(prev) === JSON.stringify(st) ? prev : st,
          ),
        ),
      5000,
    );
    return () => clearInterval(t);
  }, [refresh]);

  const showLogs = async () => setLogText(await logs(8));

  const doRepair = async () => {
    setBusy("repair");
    const r = await repairUdev();
    toaster.toast({
      title: "BC-250 Dashboard",
      body: r.ok ? "Accès à l'écran réparé" : `Échec : ${r.error}`,
    });
    if (r.ok) await restart();
    await refresh();
    setBusy("");
  };

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
    setUpdLog(r.log.split("\n").slice(-6).join("\n"));
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
  const noPanel = !!status && !status.panel;
  const serviceLabel = !status
    ? "…"
    : status.active
      ? "actif"
      : status.state === "activating"
        ? "en attente de l'écran"
        : status.state;

  return (
    <>
      {dirty && (
        <PanelSection>
          <Row>
            <ButtonItem layout="below" disabled={busy !== ""} onClick={apply}>
              {busy === "apply"
                ? "Application…"
                : `Appliquer (${Object.keys(draft).length}) et redémarrer`}
            </ButtonItem>
          </Row>
        </PanelSection>
      )}

      {noPanel && (
        <PanelSection title="⚠ Écran non détecté">
          <Row>
            {!status.usb
              ? "Aucun écran USB détecté. Vérifiez le câble (il doit transmettre des données, pas seulement charger) et le port."
              : !status.udev_rule
                ? "Écran présent, mais la règle d'accès (udev) est absente : souvent après une mise à jour de SteamOS."
                : "Écran présent, mais /dev/turzx-panel est absent."}
          </Row>
          {status.usb && status.can_repair && (
            <Row>
              <ButtonItem
                layout="below"
                disabled={busy !== ""}
                onClick={doRepair}
              >
                {busy === "repair" ? "Réparation…" : "Réparer l'accès à l'écran"}
              </ButtonItem>
            </Row>
          )}
        </PanelSection>
      )}

      <PanelSection title="État">
        <Row>
          Service : {serviceLabel}
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

      <PanelSection title="FPS">
        <Row>
          <div style={{ whiteSpace: "pre-line" }}>{fps === null
            ? "Diagnostic non lancé"
            : [
                `Overlay Steam (mangoapp) : ${fps.mangoapp ? "actif" : "inactif"}`,
                fps.mangoapp
                  ? `Config : ${fps.block ? "enregistrement FPS activé" : "enregistrement FPS absent"}`
                  : "Activez l'overlay de performances dans Steam",
                `Dernier relevé : ${fps.csv_age === null ? "aucun" : `il y a ${fps.csv_age} s`}`,
              ].join("\n")}</div>
        </Row>
        <Row>
          <ButtonItem
            layout="below"
            onClick={async () => setFps(await fpsDiag())}
          >
            Diagnostiquer les FPS
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
        {updLog && (
          <Row>
            <div style={LOG_STYLE}>{updLog}</div>
          </Row>
        )}
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
            onClick={() => (logText === null ? showLogs() : setLogText(null))}
          >
            {logText === null ? "Voir les derniers logs" : "Masquer les logs"}
          </ButtonItem>
        </Row>
        {logText !== null && (
          <>
            <Row>
              <div style={LOG_STYLE}>{logText || "(aucun log)"}</div>
            </Row>
            <Row>
              <ButtonItem layout="below" onClick={showLogs}>
                Actualiser
              </ButtonItem>
            </Row>
          </>
        )}
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
