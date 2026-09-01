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
  const [saving, setSaving] = useState(false);
  const [fieldId] = useState(() => crypto.randomUUID());
  const markFieldDirty = useSelectionStore((state) => state.markFieldDirty);
  const beginFieldSave = useSelectionStore((state) => state.beginFieldSave);
  const finishFieldSave = useSelectionStore((state) => state.finishFieldSave);
  useEffect(() => setDraft(value), [value]);

  const save = async () => {
    if (draft === value) {
      beginFieldSave(fieldId);
      finishFieldSave(fieldId, true);
      return;
    }
    setSaving(true);
    beginFieldSave(fieldId);
    try {
      await onSave(draft);
      finishFieldSave(fieldId, true);
    } catch {
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
          onChange={(event) => { setDraft(event.target.value); markFieldDirty(fieldId); }}
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
          onChange={(event) => { setDraft(event.target.value); markFieldDirty(fieldId); }}
          onFocus={onFocus}
          onBlur={() => void save()}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
        />
      )}
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
  const [fieldId] = useState(() => crypto.randomUUID());
  const markFieldDirty = useSelectionStore((state) => state.markFieldDirty);
  const beginFieldSave = useSelectionStore((state) => state.beginFieldSave);
  const finishFieldSave = useSelectionStore((state) => state.finishFieldSave);
  useEffect(() => setDraft(String(value)), [value]);
  return (
    <label className="field">
      <span>{label}</span>
      <input
        type="number"
        step={step}
        min="0"
        value={draft}
        onFocus={onFocus}
        onChange={(event) => { setDraft(event.target.value); markFieldDirty(fieldId); }}
        onBlur={async () => {
          const next = Number(draft);
          if (!Number.isFinite(next) || next === value) {
            setDraft(String(value));
            beginFieldSave(fieldId);
            finishFieldSave(fieldId, true);
            return;
          }
          beginFieldSave(fieldId);
          try {
            await onSave(next);
            finishFieldSave(fieldId, true);
          } catch {
            finishFieldSave(fieldId, false);
          }
        }}
      />
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
  const [fieldId] = useState(() => crypto.randomUUID());
  const markFieldDirty = useSelectionStore((state) => state.markFieldDirty);
  const beginFieldSave = useSelectionStore((state) => state.beginFieldSave);
  const finishFieldSave = useSelectionStore((state) => state.finishFieldSave);
  const save = async (next: string) => {
    markFieldDirty(fieldId);
    beginFieldSave(fieldId);
    try {
      await onSave(next);
      finishFieldSave(fieldId, true);
    } catch {
      finishFieldSave(fieldId, false);
    }
  };
  return (
    <label className="field">
      <span>{label}</span>
      <select
        value={value}
        onFocus={onFocus}
        onChange={(event) => void save(event.target.value)}
      >
        {options.map(([optionValue, optionLabel]) => (
          <option value={optionValue} key={optionValue}>{optionLabel}</option>
        ))}
      </select>
    </label>
  );
}
