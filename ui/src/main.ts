import { mount } from 'svelte'
import './styles/tokeny.css'
import './styles/zaklad.css'
import App from './App.svelte'

export default mount(App, { target: document.getElementById('app')! })
