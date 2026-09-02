// Browser regression fixture: first save fails, retry succeeds. No project, storage or AI calls.
import { StrictMode, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { StylePicker } from "../../components/ScriptStudio";
import { normalizeCreativeSettings } from "../creativeSettings";
import { creationTypes, styleSelections } from "../scriptStudio";
import "../../App.css";

function SaveFixture() {
  const [settings, setSettings] = useState(normalizeCreativeSettings(null));
  const [attempts, setAttempts] = useState(0);
  const count = useRef(0);
  return <main className="script-studio-stage">
    <h1>保存流程验收</h1>
    <p>首次保存失败，重试成功；不写入项目。</p>
    <StylePicker value={settings.style} contentType={settings.contentType} onChange={async (style, contentType) => {
      const attempt = ++count.current;
      setAttempts(attempt);
      await new Promise((resolve) => setTimeout(resolve, 1500));
      if (attempt === 1) throw new Error("模拟保存失败，请重试。");
      setSettings({ style, contentType });
    }} />
    <output aria-label="保存次数">{attempts}</output>
    <output aria-label="已保存设定">{[creationTypes[settings.contentType], ...styleSelections(settings.style).map((item) => item.label)].join(" · ")}</output>
  </main>;
}

createRoot(document.getElementById("root")!).render(<StrictMode><SaveFixture /></StrictMode>);
