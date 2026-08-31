import { useEffect, useState } from "react";

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
  useEffect(() => setDraft(value), [value]);

  const save = async () => {
    if (draft === value) return;
    setSaving(true);
    try {
      await onSave(draft);
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
          onChange={(event) => setDraft(event.target.value)}
          onFocus={onFocus}
          onBlur={() => void save()}
        />
      ) : (
        <input
          value={draft}
          placeholder={placeholder}
          onChange={(event) => setDraft(event.target.value)}
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
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => {
          const next = Number(draft);
          if (Number.isFinite(next) && next !== value) void onSave(next);
          else setDraft(String(value));
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
  return (
    <label className="field">
      <span>{label}</span>
      <select
        value={value}
        onFocus={onFocus}
        onChange={(event) => void onSave(event.target.value)}
      >
        {options.map(([optionValue, optionLabel]) => (
          <option value={optionValue} key={optionValue}>{optionLabel}</option>
        ))}
      </select>
    </label>
  );
}
