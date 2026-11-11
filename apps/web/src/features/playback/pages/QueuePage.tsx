import QueueScreen from "../components/QueueScreen";
import {
  PHONE_QUERY,
  useMediaQuery,
} from "../../../shared/hooks/useMediaQuery";

export default function QueuePage() {
  const phone = useMediaQuery(PHONE_QUERY);
  return phone ? <QueueScreen /> : null;
}
