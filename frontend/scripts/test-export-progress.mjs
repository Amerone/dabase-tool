import assert from 'node:assert/strict'
import { calcProgress } from '../src/utils/exportProgress.js'

const ddlOnly = { includeDdl: true, includeData: false }
const both = { includeDdl: true, includeData: true }

{
  const result = calcProgress({ ...ddlOnly, completedSteps: 1, hasError: false })
  assert.equal(result.percent, 100)
  assert.equal(result.status, 'success')
}

{
  const result = calcProgress({ ...ddlOnly, completedSteps: 0, hasError: true })
  assert.equal(result.percent, 100)
  assert.equal(result.status, 'exception')
}

{
  const result = calcProgress({ ...both, completedSteps: 1, hasError: false })
  assert.equal(result.percent, 50)
  assert.equal(result.status, 'active')
}

{
  const result = calcProgress({ ...both, completedSteps: 2, hasError: false })
  assert.equal(result.percent, 100)
  assert.equal(result.status, 'success')
}

console.log('ok')
