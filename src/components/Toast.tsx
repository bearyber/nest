import { useEffect } from "react";
import styles from "./Toast.module.css";

export interface ToastData {
  message: string;
  action?: { label: string; run: () => void };
}

const DISMISS_MS = 6000;

/** Quiet confirmation at the bottom of the window (build spec §3.2). */
export default function Toast({ toast, onDismiss }: { toast: ToastData; onDismiss: () => void }) {
  useEffect(() => {
    const t = setTimeout(onDismiss, DISMISS_MS);
    return () => clearTimeout(t);
  }, [toast, onDismiss]);

  return (
    <div className={styles.toast} role="status">
      <span>{toast.message}</span>
      {toast.action && (
        <button
          type="button"
          className={styles.action}
          onClick={() => {
            toast.action?.run();
            onDismiss();
          }}
        >
          {toast.action.label}
        </button>
      )}
    </div>
  );
}
