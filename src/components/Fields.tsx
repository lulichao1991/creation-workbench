import { useEffect, useState } from "react";
import { useSelectionStore } from "../stores/selectionStore";

interface TextFieldProps {
  label: string;
  value: string;
  multiline?: boolean;
  placeholder?: string;
  onFocus?: () => void;
  onSave: (value: string) => Promise<void> | void;
}

export function TextField({ label, value, multiline, placeholder, onFocus, onSave }: TextFieldProps) {
  const [draft, setDraft] = useState(value);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [failed, setFailed] = useState(false);
  const [fieldId] = useState(() => crypto.randomUUID());
  const markFieldDirty = useSelectionStore((state) => state.markFieldDirty);
  const beginFieldSave = useSelectionStore((state) => state.beginFieldSave);
  const finishFieldSave = useSelectionStore((state) => state.finishFieldSave);
  useEffect(() => { if (!dirty) setDraft(value); }, [value, dirty]);

  const save = async () => {
    if (draft === value) {
      beginFieldSave(fieldId);
      finishFieldSave(fieldId, true);
      return;
    }
    setSaving(true);
    setFailed(false);
    beginFieldSave(fieldId);
    try {
      await onSave(draft);
      setDirty(false);
      finishFieldSave(fieldId, true);
    } catch {
      setFailed(true);
      finishFieldSave(fieldId, false);
    } finally {
      setSaving(false);
    }
  };

  return (
    <label className={`field ${multiline ? "field-wide" : ""}`}>
      <span>{label}{saving ? " · 保存中" : ""}</span>
      {multiline ? (
        <textarea
          value={draft}
          placeholder={placeholder}
          onChange={(event) => { setDraft(event.target.value); setDirty(true); markFieldDirty(fieldId); }}
          onFocus={onFocus}
          onBlur={() => void save()}
          onKeyDown={(event) => {
            if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
              event.preventDefault();
              void save();
            }
          }}
        />
      ) : (
        <input
          value={draft}
          placeholder={placeholder}
          onChange={(event) => { setDraft(event.target.value); setDirty(true); markFieldDirty(fieldId); }}
          onFocus={onFocus}
          onBlur={() => void save()}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
        />
      )}
      {failed && <button type="button" className="field-retry" onClick={() => void save()}>保存失败，点击重试</button>}
    </label>
  );
}

interface NumberFieldProps {
  label: string;
  value: number;
  step?: number;
  onFocus?: () => void;
  onSave: (value: number) => Promise<void> | void;
}

export function NumberField({ label, value, step = 1, onFocus, onSave }: NumberFieldProps) {
  const [draft, setDraft] = useState(String(value));
  const [dirty, setDirty] = useState(false);
  const [failed, setFailed] = useState(false);
  const [fieldId] = useState(() => crypto.randomUUID());
  const markFieldDirty = useSelectionStore((state) => state.markFieldDirty);
  const beginFieldSave = useSelectionStore((state) => state.beginFieldSave);
  const finishFieldSave = useSelectionStore((state) => state.finishFieldSave);
  useEffect(() => { if (!dirty) setDraft(String(value)); }, [value, dirty]);
  return (
    <label className="field">
      <span>{label}</span>
      <input
        type="number"
        step={step}
        min="0"
        value={draft}
        onFocus={onFocus}
        onChange={(event) => { setDraft(event.target.value); setDirty(true); markFieldDirty(fieldId); }}
        onBlur={async () => {
          const next = Number(draft);
          if (!Number.isFinite(next) || next === value) {
            setDraft(String(value));
            setDirty(false);
            beginFieldSave(fieldId);
            finishFieldSave(fieldId, true);
            return;
          }
          beginFieldSave(fieldId);
          setFailed(false);
          try {
            await onSave(next);
            setDirty(false);
            finishFieldSave(fieldId, true);
          } catch {
            setFailed(true);
            finishFieldSave(fieldId, false);
          }
        }}
      />
      {failed && <button type="button" className="field-retry" onClick={(event) => { const input = event.currentTarget.parentElement?.querySelector("input"); input?.focus(); input?.blur(); }}>保存失败，点击重试</button>}
    </label>
  );
}

interface SelectFieldProps {
  label: string;
  value: string;
  options: Array<[string, string]>;
  onFocus?: () => void;
  onSave: (value: string) => Promise<void> | void;
}

export function SelectField({ label, value, options, onFocus, onSave }: SelectFieldProps) {
  const [draft, setDraft] = useState(value);
  const [dirty, setDirty] = useState(false);
  const [failed, setFailed] = useState(false);
  const [fieldId] = useState(() => crypto.randomUUID());
  const markFieldDirty = useSelectionStore((state) => state.markFieldDirty);
  const beginFieldSave = useSelectionStore((state) => state.beginFieldSave);
  const finishFieldSave = useSelectionStore((state) => state.finishFieldSave);
  useEffect(() => { if (!dirty) setDraft(value); }, [value, dirty]);
  const save = async (next: string) => {
    setDraft(next);
    setDirty(true);
    setFailed(false);
    markFieldDirty(fieldId);
    beginFieldSave(fieldId);
    try {
      await onSave(next);
      setDirty(false);
      finishFieldSave(fieldId, true);
    } catch {
      setFailed(true);
      finishFieldSave(fieldId, false);
    }
  };
  return (
    <label className="field">
      <span>{label}</span>
      <select
        value={draft}
        onFocus={onFocus}
        onChange={(event) => void save(event.target.value)}
      >
        {options.map(([optionValue, optionLabel]) => (
          <option value={optionValue} key={optionValue}>{optionLabel}</option>
        ))}
      </select>
      {failed && <button type="button" className="field-retry" onClick={() => void save(draft)}>保存失败，点击重试</button>}
    </label>
  );
}
