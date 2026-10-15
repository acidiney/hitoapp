import {
  createContext,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { X } from "lucide-react";

const NoticeContext = createContext<{
  notify: (message: string) => void;
  message: string;
} | null>(null);

export function NoticeProvider({ children }: { children: ReactNode }) {
  const [message, setMessage] = useState("");
  const value = useMemo(() => ({ notify: setMessage, message }), [message]);
  return (
    <NoticeContext.Provider value={value}>{children}</NoticeContext.Provider>
  );
}

function useNotices() {
  const notice = useContext(NoticeContext);
  if (!notice) throw new Error("Notices need a NoticeProvider");
  return notice;
}

export function useNotice() {
  return useNotices().notify;
}

export function NoticeBanner() {
  const { message, notify } = useNotices();
  if (!message) return null;
  return (
    <div className="notice" role="status">
      <span>{message}</span>
      <button
        className="icon-button"
        aria-label="Dismiss message"
        onClick={() => notify("")}
      >
        <X size={17} />
      </button>
    </div>
  );
}
