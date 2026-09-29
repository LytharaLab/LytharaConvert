<script setup>
import { computed } from 'vue';
import {
  state, setTheme, openBrowser, updateSettings, testEngine, applyEnginePaths,
  restartServer, requestNotifyPermission, toast
} from '../store.js';
import AppIcon from './AppIcon.vue';

const themes = [
  { value: 'system', label: '跟随系统' },
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' }
];

const SOURCE_LABEL = {
  custom: '设置指定',
  env: '环境变量',
  bundled: '随附 binaries',
  alongside: 'exe 同级',
  dev: '开发目录',
  path: 'PATH'
};

const sourceLabel = value => SOURCE_LABEL[value] || value || '未知';
const testState = kind => (state.engineTest.kind === kind ? state.engineTest : null);

async function toggleNotify(event) {
  const enabled = event.target.checked;
  await updateSettings({ notifyOnFinish: enabled });
  if (enabled) await requestNotifyPermission(true);
}

async function resetEngine() {
  state.settings.ffmpegPath = '';
  state.settings.ffprobePath = '';
  if (await updateSettings({ ffmpegPath: '', ffprobePath: '' })) toast('已恢复自动探测');
}

/// 端口必须先存进设置再重启，否则新进程拿到的还是旧值。
async function saveAndRestart() {
  const port = Number(state.settings.port);
  if (!Number.isInteger(port) || port < 1024 || port > 65535) {
    return toast('端口需在 1024–65535 之间');
  }
  if (!(await updateSettings({ port }))) return;
  restartServer();
}
</script>

<template>
  <div v-if="state.settingsOpen" class="modal-overlay" role="dialog" aria-modal="true" aria-label="偏好设置" @click.self="state.settingsOpen = false">
    <div class="modal settings-modal">
      <div class="modal-header">
        <h2>偏好设置</h2>
        <button class="icon-button" aria-label="关闭" @click="state.settingsOpen = false">×</button>
      </div>

      <div class="settings-body">
        <section class="setting-section">
          <h3>界面</h3>
          <div class="setting-block">
            <span class="field-label">界面主题</span>
            <div class="segmented" role="group" aria-label="界面主题">
              <button
                v-for="item in themes"
                :key="item.value"
                :class="{ chosen: state.settings.theme === item.value }"
                @click="setTheme(item.value)"
              >{{ item.label }}</button>
            </div>
          </div>
          <label class="toggle-row">
            <span><b>任务结束后通知</b><small>浏览器通知，任务跑完提醒你一声</small></span>
            <input type="checkbox" :checked="state.settings.notifyOnFinish" @change="toggleNotify" />
            <i></i>
          </label>
        </section>

        <section class="setting-section">
          <h3>输出</h3>
          <div class="setting-block">
            <span class="field-label">输出文件夹</span>
            <div class="output-location">
              <div class="output-icon">◈</div>
              <div><small>转换结果默认保存到这里</small><strong>{{ state.settings.outputDir || '未设置' }}</strong></div>
              <button class="text-button" @click="openBrowser('output')">更改</button>
            </div>
          </div>
          <label class="toggle-row">
            <span><b>保留拖放上传的原文件</b><small>关闭时，拖进来的文件在转换完成后自动删除</small></span>
            <input
              type="checkbox"
              :checked="state.settings.keepUploads"
              @change="updateSettings({ keepUploads: $event.target.checked })"
            />
            <i></i>
          </label>
        </section>

        <section class="setting-section">
          <h3>转换引擎</h3>
          <p class="setting-note">
            留空即自动探测 exe 旁的 <code>binaries\</code>。当前生效：
            <b>{{ state.engine.ffmpeg || '未找到' }}</b>（{{ sourceLabel(state.engine.ffmpegSource) }}）
          </p>
          <div class="engine-row">
            <label>ffmpeg 路径</label>
            <input v-model="state.settings.ffmpegPath" spellcheck="false" placeholder="留空 = 自动探测，例如 D:\tools\ffmpeg.exe" />
            <button class="text-button" :disabled="state.engineTest.running" @click="testEngine('ffmpeg', state.settings.ffmpegPath)">检测</button>
          </div>
          <p v-if="testState('ffmpeg')" class="engine-result" :class="{ ok: testState('ffmpeg').ok, bad: testState('ffmpeg').ok === false }">
            {{ testState('ffmpeg').running ? '检测中…' : testState('ffmpeg').message }}
          </p>
          <div class="engine-row">
            <label>ffprobe 路径</label>
            <input v-model="state.settings.ffprobePath" spellcheck="false" placeholder="留空 = 自动探测，例如 D:\tools\ffprobe.exe" />
            <button class="text-button" :disabled="state.engineTest.running" @click="testEngine('ffprobe', state.settings.ffprobePath)">检测</button>
          </div>
          <p v-if="testState('ffprobe')" class="engine-result" :class="{ ok: testState('ffprobe').ok, bad: testState('ffprobe').ok === false }">
            {{ testState('ffprobe').running ? '检测中…' : testState('ffprobe').message }}
          </p>
          <div class="engine-actions">
            <button class="text-button" @click="applyEnginePaths"><AppIcon name="check" /> 应用引擎路径</button>
            <button class="text-button subtle" @click="resetEngine">恢复自动探测</button>
          </div>
        </section>

        <section class="setting-section">
          <h3>服务</h3>
          <div class="engine-row">
            <label>端口</label>
            <input v-model.number="state.settings.port" type="number" min="1024" max="65535" />
            <button class="text-button" :disabled="state.restarting" @click="saveAndRestart">
              {{ state.restarting ? '重启中…' : '保存并重启服务' }}
            </button>
          </div>
          <p class="setting-note">
            当前监听 <code>127.0.0.1:{{ state.port }}</code>。
            <span v-if="state.restartRequired" class="warn-text">端口已改，重启后生效。</span>
          </p>
          <div class="engine-row">
            <label>并发任务</label>
            <input
              v-model.number="state.settings.concurrency"
              type="number"
              min="1"
              max="8"
              @change="updateSettings({ concurrency: Number(state.settings.concurrency) })"
            />
            <span class="row-hint">1 = 逐个处理（最省资源）</span>
          </div>
          <label class="toggle-row">
            <span><b>启动时自动打开浏览器</b><small>关闭后需要自己访问 http://127.0.0.1:{{ state.settings.port }}/</small></span>
            <input
              type="checkbox"
              :checked="state.settings.openBrowser"
              @change="updateSettings({ openBrowser: $event.target.checked })"
            />
            <i></i>
          </label>
        </section>
      </div>

      <p class="settings-footer">媒体文件只在你的电脑上处理，不会上传到任何服务器。</p>
    </div>
  </div>
</template>
