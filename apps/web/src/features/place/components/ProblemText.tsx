import type { ConnectProblem } from "../hooks/useConnectServer";

export default function ProblemText({ problem }: { problem: ConnectProblem }) {
  switch (problem.outcome) {
    case "not_an_address":
      return <>Type an address like hito.home:8080 or 192.168.1.20:8080.</>;
    case "no_answer":
      return (
        <>
          No answer from {problem.shown}. Check the address, and that this
          device is on the same network or VPN.
        </>
      );
    case "not_hito":
      return <>{problem.host} answered, but it is not a Hito server.</>;
    case "certificate":
      return (
        <>
          The certificate for {problem.host} can't be checked, so Hito won't
          send your password there.
          <span className="connect-detail">
            Give the server a certificate this device trusts, or use http://
            inside your VPN.
          </span>
        </>
      );
    case "server_too_old":
      return (
        <>
          This server runs{" "}
          {problem.version ? `Hito ${problem.version}` : "an older Hito"}, which
          is too old for this app. Update the server, or open it in a browser.
        </>
      );
    case "app_too_old":
      return <>This app is older than the server. Update the app.</>;
    case "failed":
      return <>Hito couldn't open this server. Try again.</>;
  }
}
