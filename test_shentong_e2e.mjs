import { chromium } from 'playwright';

const BASE = process.env.E2E_BASE_URL ?? 'http://localhost:5173';
const password = process.env.SHENTONG_PASSWORD ?? process.env.SHENTONG_DIAG_PASSWORD;

if (!password) {
  console.error('Set SHENTONG_PASSWORD or SHENTONG_DIAG_PASSWORD before running this E2E test');
  process.exit(2);
}

const SHENTONG_CONFIG = {
  host: process.env.SHENTONG_HOST ?? '127.0.0.1',
  port: process.env.SHENTONG_PORT ?? '2003',
  username: process.env.SHENTONG_USER ?? 'SYSDBA',
  password,
  schema: process.env.SHENTONG_SCHEMA ?? process.env.SHENTONG_USER ?? 'SYSDBA',
};

async function sleep(ms) {
  return new Promise(r => setTimeout(r, ms));
}

(async () => {
  const browser = await chromium.launch({ headless: false, slowMo: 300 });
  const page = await browser.newPage();

  console.log('=== Step 1: Open app ===');
  await page.goto(BASE);
  await page.waitForLoadState('networkidle');
  await page.screenshot({ path: 'screenshots/01_home.png' });
  console.log('App loaded');

  // Step 2: Select ShenTong database type
  console.log('\n=== Step 2: Select ShenTong database type ===');

  // Click the database type selector
  const dbTypeSelector = page.locator('.ant-select').first();
  await dbTypeSelector.click();
  await sleep(500);

  // Select ShenTong/OSCAR option
  const shentongOption = page.getByText('ShenTong/OSCAR');
  if (await shentongOption.count() > 0) {
    await shentongOption.click();
    console.log('Selected ShenTong/OSCAR');
  } else {
    console.log('ERROR: ShenTong/OSCAR option not found in dropdown');
    // List all visible options for debugging
    const options = page.locator('.ant-select-item-option');
    const count = await options.count();
    for (let i = 0; i < count; i++) {
      console.log(`  Option ${i}: ${await options.nth(i).textContent()}`);
    }
    await page.screenshot({ path: 'screenshots/02_no_shentong_option.png' });
    await browser.close();
    process.exit(1);
  }

  await sleep(500);
  await page.screenshot({ path: 'screenshots/02_db_type_selected.png' });

  // Step 3: Fill in connection details
  console.log('\n=== Step 3: Fill connection form ===');

  // Clear and fill host
  const hostInput = page.locator('input#host');
  await hostInput.click();
  await hostInput.fill(SHENTONG_CONFIG.host);

  // Port should auto-fill to 2003 when ShenTong is selected, but let's verify and fill
  const portInput = page.locator('input#port');
  const currentPort = await portInput.inputValue();
  console.log(`Current port value: ${currentPort}`);
  if (currentPort !== '2003') {
    await portInput.click();
    await portInput.fill(SHENTONG_CONFIG.port);
  }

  // Fill username
  const usernameInput = page.locator('input#username');
  await usernameInput.click();
  await usernameInput.fill(SHENTONG_CONFIG.username);

  // Fill password
  const passwordInput = page.locator('input#password');
  await passwordInput.click();
  await passwordInput.fill(SHENTONG_CONFIG.password);

  // Fill schema
  const schemaInput = page.locator('input#schema');
  await schemaInput.click();
  await schemaInput.fill(SHENTONG_CONFIG.schema);

  await page.screenshot({ path: 'screenshots/03_form_filled.png' });
  console.log('Connection form filled');

  // Step 4: Test connection
  console.log('\n=== Step 4: Test connection ===');

  // Look for the test connection button
  const testBtn = page.getByRole('button', { name: /测试连接|Test/i });
  if (await testBtn.count() === 0) {
    console.log('Test button not found by role, trying text match...');
    const allButtons = page.locator('button');
    const btnCount = await allButtons.count();
    for (let i = 0; i < btnCount; i++) {
      const text = await allButtons.nth(i).textContent();
      console.log(`  Button ${i}: "${text}"`);
    }
  }

  await testBtn.click();
  console.log('Clicked test connection button');

  // Wait for response - look for success or error message
  await sleep(5000); // Shentong connection may take time
  await page.screenshot({ path: 'screenshots/04_connection_result.png' });

  // Check for success/error messages
  const successMsg = page.locator('.ant-message-success, .ant-message-notice');
  const errorMsg = page.locator('.ant-message-error');

  if (await errorMsg.count() > 0) {
    const errorText = await errorMsg.first().textContent();
    console.log(`CONNECTION ERROR: ${errorText}`);
    // Take additional screenshot and continue to debug
    await page.screenshot({ path: 'screenshots/04_connection_error.png' });
  } else if (await successMsg.count() > 0) {
    const successText = await successMsg.first().textContent();
    console.log(`CONNECTION SUCCESS: ${successText}`);
  } else {
    console.log('No clear success/error message detected, checking page state...');
  }

  // Step 5: Try to proceed to next step (schema/table selection)
  console.log('\n=== Step 5: Navigate to schema/table selection ===');
  await sleep(2000);

  // Look for "Next" or "下一步" button
  const nextBtn = page.getByRole('button', { name: /下一步|Next|获取|加载/i });
  if (await nextBtn.count() > 0) {
    await nextBtn.click();
    console.log('Clicked next/load button');
    await sleep(3000);
    await page.screenshot({ path: 'screenshots/05_schema_tables.png' });
  } else {
    console.log('No next button found, looking at current page state...');
    await page.screenshot({ path: 'screenshots/05_current_state.png' });
  }

  // Step 6: Check if tables are loaded
  console.log('\n=== Step 6: Check table list ===');
  await sleep(2000);

  // Look for table checkboxes or table list
  const tableItems = page.locator('.ant-checkbox-wrapper, .ant-table-row, .ant-tree-treenode');
  const tableCount = await tableItems.count();
  console.log(`Found ${tableCount} table/tree items`);

  if (tableCount > 0) {
    // Select first few tables for export
    for (let i = 0; i < Math.min(3, tableCount); i++) {
      const text = await tableItems.nth(i).textContent();
      console.log(`  Item ${i}: ${text}`);
    }
  }

  await page.screenshot({ path: 'screenshots/06_table_list.png' });

  // Step 7: Try DDL export if we have tables
  console.log('\n=== Step 7: Attempt DDL export ===');

  // Select all tables if there's a "select all" option
  const selectAll = page.locator('text=全选').first();
  if (await selectAll.count() > 0) {
    await selectAll.click();
    console.log('Clicked select all');
    await sleep(1000);
  }

  // Look for export button
  const exportBtn = page.getByRole('button', { name: /导出|Export|DDL/i });
  if (await exportBtn.count() > 0) {
    await exportBtn.first().click();
    console.log('Clicked export button');
    await sleep(5000);
    await page.screenshot({ path: 'screenshots/07_export_result.png' });
  } else {
    console.log('No export button found on current page');
    await page.screenshot({ path: 'screenshots/07_no_export.png' });
  }

  // Final state
  console.log('\n=== Final State ===');
  await page.screenshot({ path: 'screenshots/08_final.png' });

  // Keep browser open for 10 seconds for manual inspection
  console.log('Keeping browser open for 10 seconds...');
  await sleep(10000);

  await browser.close();
  console.log('\n=== Test complete ===');
})();
