import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import { applyTheme, cachedTheme } from "./lib/theme";

applyTheme(cachedTheme());

const target = document.getElementById("app");
if (!target) throw new Error("missing #app element");

export default mount(App, { target });
