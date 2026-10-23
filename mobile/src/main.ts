import { mount } from "svelte";
import "../../src/lib/themeTokens.css";
import "./fonts.css";
import "./base.css";
import "./markdown.css";
import App from "./App.svelte";

mount(App, { target: document.getElementById("app")! });
