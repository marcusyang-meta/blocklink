import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from 'typescript';
const input=fs.readFileSync('src/i18n.tsx','utf8').replace("import english from './locales/en.json';",'const english='+fs.readFileSync('src/locales/en.json','utf8')+';');
const compiled=ts.transpileModule(input,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.React}});
// Resolve React from this project while testing the actual TypeScript module.
const temporary=new URL('../.i18n-test.mjs',import.meta.url);
fs.writeFileSync(temporary,compiled.outputText);
const i18n=await import(temporary.href);
fs.unlinkSync(temporary);
const catalog=JSON.parse(fs.readFileSync('src/locales/en.json','utf8'));
test('locale selection and interpolation preserve user data',()=>{
 assert.equal(i18n.resolveLocale('en',['zh-CN']),'en');
 assert.equal(i18n.resolveLocale(null,['zh-TW']),'zh-CN');
 assert.equal(i18n.resolveLocale('invalid',['fr']),'en');
 assert.equal(i18n.translate('选择 {0}','en',['中文世界']), 'Select 中文世界');
 assert.equal(i18n.translate(' 我的游戏 ','en'),' My games ');
 assert.equal(i18n.translate('My own world','en'),'My own world');
 assert.equal(i18n.translate('删除游戏','zh-CN'),'删除游戏');
});
test('every UI translation key exists and placeholders agree',()=>{
 for(const [zh,en] of Object.entries(catalog)){
  assert.deepEqual(zh.match(/\{\d+\}/g)||[],en.match(/\{\d+\}/g)||[],zh);
  assert.ok(!/[\u3400-\u9fff]/.test(en),zh);
 }
 for(const file of fs.readdirSync('src').filter(f=>/\.tsx?$/.test(f)&&f!=='i18n.tsx')){
  const source=ts.createSourceFile(file,fs.readFileSync('src/'+file,'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
  function walk(n){
   if(ts.isCallExpression(n)&&n.expression.getText(source)==='t'&&ts.isStringLiteral(n.arguments[0]))assert.ok(Object.hasOwn(catalog,n.arguments[0].text.trim()),file+': '+n.arguments[0].text);
   if(ts.isJsxText(n))assert.ok(!/[\u3400-\u9fff]/.test(n.text),file+': untranslated JSX');
   ts.forEachChild(n,walk);
  }walk(source);
 }
});
