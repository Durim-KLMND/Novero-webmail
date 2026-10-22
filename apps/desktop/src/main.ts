import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import { applyTheme, cachedTheme } from "./lib/theme";
import { watchTitlebar } from "./lib/frame.svelte";
import { guardModalScroll } from "./lib/modal";
import { watchPlan } from "./lib/pro.svelte";

applyTheme(cachedTheme());
watchTitlebar();
watchPlan();
guardModalScroll();

const target = document.getElementById("app");
if (!target) throw new Error("missing #app element");

export default mount(App, { target });
