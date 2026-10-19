import { useState, type FormEvent } from "react";
import { createRoot } from "react-dom/client";

function ControllerForm() {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [result, setResult] = useState<"idle" | "ok" | "bad">("idle");

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setResult(
      username === "react-user" && password === "react-pass" ? "ok" : "bad",
    );
  }

  return (
    <form id="loginForm" onSubmit={onSubmit}>
      <label htmlFor="username">Username</label>
      <input
        id="username"
        autoComplete="off"
        value={username}
        onChange={(event) => setUsername(event.target.value)}
      />
      <label htmlFor="password">Password</label>
      <input
        id="password"
        type="password"
        autoComplete="off"
        value={password}
        onChange={(event) => setPassword(event.target.value)}
      />
      <button id="submit" type="submit">
        Sign in
      </button>
      <div id="state">
        {username}|{password.length}
      </div>
      {result === "ok" ? <div id="success">REACT_OK</div> : null}
      {result === "bad" ? <div id="error">REACT_BAD_CREDENTIALS</div> : null}
    </form>
  );
}

createRoot(document.getElementById("root")!).render(<ControllerForm />);
