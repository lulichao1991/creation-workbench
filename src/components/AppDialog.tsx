import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";

export interface DialogOption {
  value: string;
  label: string;
  description?: string;
}

interface PromptOptions {
  label?: string;
  defaultValue?: string;
  placeholder?: string;
  multiline?: boolean;
  options?: DialogOption[];
  optional?: boolean;
  confirmLabel?: string;
}

interface ConfirmOptions {
  title?: string;
  confirmLabel?: string;
  danger?: boolean;
}

export interface DialogApi {
  prompt: (title: string, options?: PromptOptions) => Promise<string | null>;
  confirm: (message: string, options?: ConfirmOptions) => Promise<boolean>;
  alert: (message: string, title?: string) => Promise<void>;
}

type Request =
  | { kind: "prompt"; title: string; options: PromptOptions; resolve: (value: string | null) => void }
  | { kind: "confirm"; message: string; options: ConfirmOptions; resolve: (value: boolean) => void }
  | { kind: "alert"; message: string; title: string; resolve: () => void };

const DialogContext = createContext<DialogApi | null>(null);

export function AppDialogProvider({ children }: { children: ReactNode }) {
  const [requests, setRequests] = useState<Request[]>([]);
  const [value, setValue] = useState("");
  const fieldRef = useRef<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement | null>(null);
  const dialogRef = useRef<HTMLElement | null>(null);
  const requestsRef = useRef<Request[]>([]);
  const request = requests[0] ?? null;

  const enqueue = useCallback((next: Request) => {
    requestsRef.current = [...requestsRef.current, next];
    setRequests(requestsRef.current);
  }, []);
  const dequeue = useCallback(() => {
    requestsRef.current = requestsRef.current.slice(1);
    setRequests(requestsRef.current);
  }, []);

  const prompt = useCallback<DialogApi["prompt"]>((title, options = {}) => new Promise((resolve) => {
    enqueue({ kind: "prompt", title, options, resolve });
  }), [enqueue]);
  const confirm = useCallback<DialogApi["confirm"]>((message, options = {}) => new Promise((resolve) => {
    enqueue({ kind: "confirm", message, options, resolve });
  }), [enqueue]);
  const alert = useCallback<DialogApi["alert"]>((message, title = "提示") => new Promise((resolve) => {
    enqueue({ kind: "alert", message, title, resolve });
  }), [enqueue]);

  const cancel = useCallback(() => {
    if (!request) return;
    if (request.kind === "prompt") request.resolve(null);
    else if (request.kind === "confirm") request.resolve(false);
    else request.resolve();
    dequeue();
  }, [request, dequeue]);

  const submit = useCallback(() => {
    if (!request) return;
    if (request.kind === "prompt") request.resolve(value.trim());
    else if (request.kind === "confirm") request.resolve(true);
    else request.resolve();
    dequeue();
  }, [request, value, dequeue]);

  useEffect(() => {
    if (request?.kind === "prompt") {
      setValue(request.options.defaultValue ?? request.options.options?.[0]?.value ?? "");
    }
  }, [request]);

  useEffect(() => () => {
    for (const pending of requestsRef.current) {
      if (pending.kind === "prompt") pending.resolve(null);
      else if (pending.kind === "confirm") pending.resolve(false);
      else pending.resolve();
    }
    requestsRef.current = [];
  }, []);

  useEffect(() => {
    if (!request) return;
    const timer = window.setTimeout(() => (fieldRef.current ?? dialogRef.current?.querySelector<HTMLElement>("button"))?.focus(), 0);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") cancel();
      if (event.key === "Tab" && dialogRef.current) {
        const focusable = [...dialogRef.current.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled])")];
        if (!focusable.length) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => { window.clearTimeout(timer); window.removeEventListener("keydown", onKeyDown); };
  }, [request, cancel]);

  const promptOptions = request?.kind === "prompt" ? request.options : null;
  const submitDisabled = Boolean(request?.kind === "prompt" && !promptOptions?.optional && !value.trim());

  return (
    <DialogContext.Provider value={{ prompt, confirm, alert }}>
      {children}
      {request && (
        <div className="dialog-backdrop" onMouseDown={(event) => event.target === event.currentTarget && cancel()}>
          <section ref={dialogRef} className="app-dialog" role="dialog" aria-modal="true" aria-labelledby="app-dialog-title">
            <div className="dialog-heading">
              <h2 id="app-dialog-title">{request.kind === "confirm" ? request.options.title ?? "请确认" : request.kind === "alert" ? request.title : request.title}</h2>
              {request.kind !== "prompt" && <p>{request.message}</p>}
            </div>
            {request.kind === "prompt" && (
              <label className="dialog-field">
                <span>{request.options.label ?? "请输入"}{request.options.optional ? "（可选）" : ""}</span>
                {request.options.options ? (
                  <select ref={(node) => { fieldRef.current = node; }} value={value} onChange={(event) => setValue(event.target.value)}>
                    {request.options.options.map((option) => <option value={option.value} key={option.value}>{option.label}{option.description ? ` — ${option.description}` : ""}</option>)}
                  </select>
                ) : request.options.multiline ? (
                  <textarea ref={(node) => { fieldRef.current = node; }} value={value} placeholder={request.options.placeholder} onChange={(event) => setValue(event.target.value)} rows={5} />
                ) : (
                  <input ref={(node) => { fieldRef.current = node; }} value={value} placeholder={request.options.placeholder} onChange={(event) => setValue(event.target.value)} onKeyDown={(event) => event.key === "Enter" && !submitDisabled && submit()} />
                )}
              </label>
            )}
            <div className="dialog-actions">
              {request.kind !== "alert" && <button className="ghost" onClick={cancel}>取消</button>}
              <button className={request.kind === "confirm" && request.options.danger ? "dialog-danger" : "primary"} disabled={submitDisabled} onClick={submit}>
                {request.kind === "prompt" ? request.options.confirmLabel ?? "确定" : request.kind === "confirm" ? request.options.confirmLabel ?? "确认" : "知道了"}
              </button>
            </div>
          </section>
        </div>
      )}
    </DialogContext.Provider>
  );
}

export function useAppDialog() {
  const context = useContext(DialogContext);
  if (!context) throw new Error("useAppDialog must be used within AppDialogProvider");
  return context;
}
