//-----------------------------------------------------------------------------
// Copyright (c) 2016, 2020, Oracle and/or its affiliates. All rights reserved.
// This program is free software: you can modify it and/or redistribute it
// under the terms of:
//
// (i)  the Universal Permissive License v 1.0 or at your option, any
//      later version (http://oss.oracle.com/licenses/upl); and/or
//
// (ii) the Apache License v 2.0. (http://www.apache.org/licenses/LICENSE-2.0)
//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------
// dpiContext.c
//   Implementation of context. Each context uses a specific version of the
// ODPI-C library, which is checked for compatibility before allowing its use.
//-----------------------------------------------------------------------------

#include "dpiImpl.h"

#define BRANCH_NUMBER_STR "020"   // branch number
#define MODULE_NAME_STR "stodpi"

#ifndef RELEASE_VERSION
#define RELEASE_VERSION	 "4.0.0-DEV"
#endif

#ifdef OS_64_BIT
#define VERSION_PLATFORM_BIT_STR "64bit"
#else 
#define VERSION_PLATFORM_BIT_STR "32bit"
#endif

#ifdef WIN32
#define VERSION_PLATFORM_STR "for Windows"
#endif

#ifdef LINUX
#define VERSION_PLATFORM_STR "for Linux"
#endif

#ifdef IA
#define VERSION_PLATFORM_STR "for IA"
#endif

#ifdef SHENWEI
#define VERSION_PLATFORM_STR "for SHENWEI"
#endif

#ifdef FEITENG
#define VERSION_PLATFORM_STR "for FEITENG"
#endif

#ifdef MIPSEL
#define VERSION_PLATFORM_STR "for MIPSEL"
#endif

#ifdef SOLARIS
#define VERSION_PLATFORM_STR "for SOLARIS"
#endif

#ifdef AIX
#define VERSION_PLATFORM_STR "for AIX"
#endif

#ifdef HPUNIX
#define VERSION_PLATFORM_STR "for HPUNIX"
#endif

// forward declarations of internal functions only used in this file
static void dpiContext__free(dpiContext *context);


//-----------------------------------------------------------------------------
// dpiContext__create() [INTERNAL]
//   Helper function for dpiContext__create().
//-----------------------------------------------------------------------------
static int dpiContext__create(const char *fnName, unsigned int majorVersion,
        unsigned int minorVersion, dpiContextCreateParams *params,
        dpiContext **context, dpiError *error)
{
    dpiVersionInfo *versionInfo;
    dpiContext *tempContext;

    // ensure global infrastructure is initialized
    if (dpiGlobal__ensureInitialized(fnName, params, &versionInfo, error) < 0)
        return DPI_FAILURE;

    // validate context handle
    if (!context)
        return dpiError__set(error, "check context handle",
                DPI_ERR_NULL_POINTER_PARAMETER, "context");

    // verify that the supplied version is supported by the library
    if (majorVersion != DPI_MAJOR_VERSION || minorVersion > DPI_MINOR_VERSION)
        return dpiError__set(error, "check version",
                DPI_ERR_VERSION_NOT_SUPPORTED, majorVersion, majorVersion,
                minorVersion, DPI_MAJOR_VERSION, DPI_MINOR_VERSION);

    // allocate context and initialize it
    if (dpiGen__allocate(DPI_HTYPE_CONTEXT, NULL, (void**) &tempContext,
            error) < 0)
        return DPI_FAILURE;
    tempContext->dpiMinorVersion = (uint8_t) minorVersion;
    tempContext->versionInfo = versionInfo;

    // store default encoding, if applicable
    if (params->defaultEncoding) {
        if (dpiUtils__allocateMemory(1, strlen(params->defaultEncoding) + 1, 0,
                "allocate default encoding",
                (void**) &tempContext->defaultEncoding, error) < 0) {
            dpiContext__free(tempContext);
            return DPI_FAILURE;
        }
        strcpy(tempContext->defaultEncoding, params->defaultEncoding);
    }

    // store default driver name, if applicable
    if (params->defaultDriverName) {
        if (dpiUtils__allocateMemory(1, strlen(params->defaultDriverName) + 1,
                0, "allocate default driver name",
                (void**) &tempContext->defaultDriverName, error) < 0) {
            dpiContext__free(tempContext);
            return DPI_FAILURE;
        }
        strcpy(tempContext->defaultDriverName, params->defaultDriverName);
    }

    *context = tempContext;
    return DPI_SUCCESS;
}


//-----------------------------------------------------------------------------
// dpiContext__free() [INTERNAL]
//   Free the memory and any resources associated with the context.
//-----------------------------------------------------------------------------
static void dpiContext__free(dpiContext *context)
{
    if (context->defaultDriverName) {
        dpiUtils__freeMemory((void*) context->defaultDriverName);
        context->defaultDriverName = NULL;
    }
    if (context->defaultEncoding) {
        dpiUtils__freeMemory((void*) context->defaultEncoding);
        context->defaultEncoding = NULL;
    }
    dpiUtils__freeMemory(context);
}


//-----------------------------------------------------------------------------
// dpiContext__initCommonCreateParams() [INTERNAL]
//   Initialize the common connection/pool creation parameters to default
// values.
//-----------------------------------------------------------------------------
void dpiContext__initCommonCreateParams(const dpiContext *context,
        dpiCommonCreateParams *params)
{
    memset(params, 0, sizeof(dpiCommonCreateParams));
    if (context->defaultEncoding) {
        params->encoding = context->defaultEncoding;
        params->nencoding = context->defaultEncoding;
    } else {
#if defined WIN32 || defined WIN64
		params->encoding = DPI_CHARSET_NAME_ZHS16GBK;
		params->nencoding = DPI_CHARSET_NAME_ZHS16GBK;
#else
		params->encoding = DPI_CHARSET_NAME_UTF8;
		params->nencoding = DPI_CHARSET_NAME_UTF8;
#endif
    }
    if (context->defaultDriverName) {
        params->driverName = context->defaultDriverName;
        params->driverNameLength =
                (uint32_t) strlen(context->defaultDriverName);
    } else {
        params->driverName = DPI_DEFAULT_DRIVER_NAME;
        params->driverNameLength = (uint32_t) strlen(params->driverName);
    }
    params->stmtCacheSize = DPI_DEFAULT_STMT_CACHE_SIZE;
}


//-----------------------------------------------------------------------------
// dpiContext__initConnCreateParams() [INTERNAL]
//   Initialize the connection creation parameters to default values. Return
// the structure size as a convenience for calling functions which may have to
// differentiate between different ODPI-C application versions.
//-----------------------------------------------------------------------------
void dpiContext__initConnCreateParams(dpiConnCreateParams *params)
{
    memset(params, 0, sizeof(dpiConnCreateParams));
}


//-----------------------------------------------------------------------------
// dpiContext__initPoolCreateParams() [INTERNAL]
//   Initialize the pool creation parameters to default values.
//-----------------------------------------------------------------------------
void dpiContext__initPoolCreateParams(dpiPoolCreateParams *params)
{
    memset(params, 0, sizeof(dpiPoolCreateParams));
    params->minSessions = 1;
    params->maxSessions = 1;
    params->sessionIncrement = 0;
    params->homogeneous = 1;
    params->getMode = DPI_MODE_POOL_GET_NOWAIT;
    params->pingInterval = DPI_DEFAULT_PING_INTERVAL;
    params->pingTimeout = DPI_DEFAULT_PING_TIMEOUT;
}


//-----------------------------------------------------------------------------
// dpiContext__initSodaOperOptions() [INTERNAL]
//   Initialize the SODA operation options to default values.
//-----------------------------------------------------------------------------
void dpiContext__initSodaOperOptions(dpiSodaOperOptions *options)
{
    memset(options, 0, sizeof(dpiSodaOperOptions));
}


//-----------------------------------------------------------------------------
// dpiContext__initSubscrCreateParams() [INTERNAL]
//   Initialize the subscription creation parameters to default values.
//-----------------------------------------------------------------------------
void dpiContext__initSubscrCreateParams(dpiSubscrCreateParams *params)
{
    memset(params, 0, sizeof(dpiSubscrCreateParams));
    params->subscrNamespace = DPI_SUBSCR_NAMESPACE_DBCHANGE;
    params->groupingType = DPI_SUBSCR_GROUPING_TYPE_SUMMARY;
}


//-----------------------------------------------------------------------------
// dpiContext_createWithParams() [PUBLIC]
//   Create a new context for interaction with the library. The major versions
// must match and the minor version of the caller must be less than or equal to
// the minor version compiled into the library. The supplied parameters can be
// used to modify how the Oracle client library is loaded.
//-----------------------------------------------------------------------------
int dpiContext_createWithParams(unsigned int majorVersion,
        unsigned int minorVersion, dpiContextCreateParams *params,
        dpiContext **context, dpiErrorInfo *errorInfo)
{
    dpiContextCreateParams localParams;
    dpiErrorInfo localErrorInfo;
    dpiError error;
    int status;

    // make a copy of the parameters so that the addition of defaults doesn't
    // modify the original parameters that were passed; then add defaults, if
    // needed
    if (params) {
        memcpy(&localParams, params, sizeof(localParams));
    } else {
        memset(&localParams, 0, sizeof(localParams));
    }
    if (!localParams.loadErrorUrl)
        localParams.loadErrorUrl = DPI_DEFAULT_LOAD_ERROR_URL;

    if (dpiDebugLevel & DPI_DEBUG_LEVEL_FNS)
        dpiDebug__print("fn start %s\n", __func__);
    status = dpiContext__create(__func__, majorVersion, minorVersion,
            &localParams, context, &error);
    if (status < 0) {
        dpiError__getInfo(&error, &localErrorInfo);
        memcpy(errorInfo, &localErrorInfo, sizeof(dpiErrorInfo__v33));
    }
    if (dpiDebugLevel & DPI_DEBUG_LEVEL_FNS)
        dpiDebug__print("fn end %s -> %d\n", __func__, status);
    return status;
}


//-----------------------------------------------------------------------------
// dpiContext_destroy() [PUBLIC]
//   Destroy an existing context. The structure will be checked for validity
// first.
//-----------------------------------------------------------------------------
int dpiContext_destroy(dpiContext *context)
{
    char message[80];
    dpiError error;

    if (dpiGen__startPublicFn(context, DPI_HTYPE_CONTEXT, __func__,
            &error) < 0)
        return dpiGen__endPublicFn(context, DPI_FAILURE, &error);
    dpiUtils__clearMemory(&context->checkInt, sizeof(context->checkInt));
    if (dpiDebugLevel & DPI_DEBUG_LEVEL_REFS)
        dpiDebug__print("ref %p (%s) -> 0\n", context, context->typeDef->name);
    if (dpiDebugLevel & DPI_DEBUG_LEVEL_FNS)
        (void) sprintf(message, "fn end %s(%p) -> %d", __func__, context,
                DPI_SUCCESS);
    dpiContext__free(context);
    if (dpiDebugLevel & DPI_DEBUG_LEVEL_FNS)
        dpiDebug__print("%s\n", message);
    return DPI_SUCCESS;
}


//-----------------------------------------------------------------------------
// dpiContext_getClientVersion() [PUBLIC]
//   Return the version of the Oracle client that is in use.
//-----------------------------------------------------------------------------
int dpiContext_getClientVersion(const dpiContext *context,
        dpiVersionInfo *versionInfo)
{
    dpiError error;

    if (dpiGen__startPublicFn(context, DPI_HTYPE_CONTEXT, __func__,
            &error) < 0)
        return dpiGen__endPublicFn(context, DPI_FAILURE, &error);
    DPI_CHECK_PTR_NOT_NULL(context, versionInfo)
    memcpy(versionInfo, context->versionInfo, sizeof(dpiVersionInfo));
    return dpiGen__endPublicFn(context, DPI_SUCCESS, &error);
}


//-----------------------------------------------------------------------------
// dpiContext_getError() [PUBLIC]
//   Return information about the error that was last populated.
//-----------------------------------------------------------------------------
void dpiContext_getError(const dpiContext *context, dpiErrorInfo *info)
{
    dpiError error;

    dpiGlobal__initError(NULL, &error);
    dpiGen__checkHandle(context, DPI_HTYPE_CONTEXT, "check handle", &error);
    dpiError__getInfo(&error, info);
}


//-----------------------------------------------------------------------------
// dpiContext_initCommonCreateParams() [PUBLIC]
//   Initialize the common connection/pool creation parameters to default
// values.
//-----------------------------------------------------------------------------
int dpiContext_initCommonCreateParams(const dpiContext *context,
        dpiCommonCreateParams *params)
{
    dpiCommonCreateParams localParams;
    dpiError error;

    if (dpiGen__startPublicFn(context, DPI_HTYPE_CONTEXT, __func__,
            &error) < 0)
        return dpiGen__endPublicFn(context, DPI_FAILURE, &error);
    DPI_CHECK_PTR_NOT_NULL(context, params)

    // size changed in version 4.2; local structure and check can be dropped
    // once version 5 released
    if (context->dpiMinorVersion > 1) {
        dpiContext__initCommonCreateParams(context, params);
    } else {
        dpiContext__initCommonCreateParams(context, &localParams);
        memcpy(params, &localParams, sizeof(dpiCommonCreateParams__v41));
    }

    return dpiGen__endPublicFn(context, DPI_SUCCESS, &error);
}


//-----------------------------------------------------------------------------
// dpiContext_initConnCreateParams() [PUBLIC]
//   Initialize the connection creation parameters to default values.
//-----------------------------------------------------------------------------
int dpiContext_initConnCreateParams(const dpiContext *context,
        dpiConnCreateParams *params)
{
    dpiError error;

    if (dpiGen__startPublicFn(context, DPI_HTYPE_CONTEXT, __func__,
            &error) < 0)
        return dpiGen__endPublicFn(context, DPI_FAILURE, &error);
    DPI_CHECK_PTR_NOT_NULL(context, params)

    dpiContext__initConnCreateParams(params);
    return dpiGen__endPublicFn(context, DPI_SUCCESS, &error);
}


//-----------------------------------------------------------------------------
// dpiContext_initPoolCreateParams() [PUBLIC]
//   Initialize the pool creation parameters to default values.
//-----------------------------------------------------------------------------
int dpiContext_initPoolCreateParams(const dpiContext *context,
        dpiPoolCreateParams *params)
{
    dpiError error;

    if (dpiGen__startPublicFn(context, DPI_HTYPE_CONTEXT, __func__,
            &error) < 0)
        return dpiGen__endPublicFn(context, DPI_FAILURE, &error);
    DPI_CHECK_PTR_NOT_NULL(context, params)

    dpiContext__initPoolCreateParams(params);
    return dpiGen__endPublicFn(context, DPI_SUCCESS, &error);
}


//-----------------------------------------------------------------------------
// dpiContext_initSodaOperOptions() [PUBLIC]
//   Initialize the SODA operation options to default values.
//-----------------------------------------------------------------------------
int dpiContext_initSodaOperOptions(const dpiContext *context,
        dpiSodaOperOptions *options)
{
    dpiSodaOperOptions localOptions;
    dpiError error;

    if (dpiGen__startPublicFn(context, DPI_HTYPE_CONTEXT, __func__,
            &error) < 0)
        return dpiGen__endPublicFn(context, DPI_FAILURE, &error);
    DPI_CHECK_PTR_NOT_NULL(context, options)

    // size changed in version 4.2; local structure and check can be dropped
    // once version 5 released
    if (context->dpiMinorVersion > 1) {
        dpiContext__initSodaOperOptions(options);
    } else {
        dpiContext__initSodaOperOptions(&localOptions);
        memcpy(options, &localOptions, sizeof(dpiSodaOperOptions__v41));
    }

    return dpiGen__endPublicFn(context, DPI_SUCCESS, &error);
}


//-----------------------------------------------------------------------------
// dpiContext_initSubscrCreateParams() [PUBLIC]
//   Initialize the subscription creation parameters to default values.
//-----------------------------------------------------------------------------
int dpiContext_initSubscrCreateParams(const dpiContext *context,
        dpiSubscrCreateParams *params)
{
    dpiError error;

    if (dpiGen__startPublicFn(context, DPI_HTYPE_CONTEXT, __func__,
            &error) < 0)
        return dpiGen__endPublicFn(context, DPI_FAILURE, &error);
    DPI_CHECK_PTR_NOT_NULL(context, params)

    dpiContext__initSubscrCreateParams(params);
    return dpiGen__endPublicFn(context, DPI_SUCCESS, &error);
}

void getStodpiVersion(char* ver)
{

	int result = DPI_SUCCESS;
	char ret[256] = { 0 };
	char build_name[8] = { 0 };
	int major_version = 0;
	int minor_version = 0;
	int key = 10;
	int month = 0;
	int day = 0;
	int year = 0;
	int i = 0, j = 0, flag = 0, len = 0;
	char *Current_Data = NULL;
	char *tmpfree1 = NULL;
	char *tmp_P = NULL;
	char tmp[20] = { 0 };
	char *monthlist[12] = { "Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec" };

	// get build time
	Current_Data = strdup(__DATE__);
	len = strlen(Current_Data);
	tmpfree1 = Current_Data;
	tmp_P = tmp;

	//ת������
	for (i = 0, j = 0; i < len + 1; i++, Current_Data++)
	{
		if (*Current_Data == ' ' || *Current_Data == '\0')
		{
			if (*tmp_P != 0)
			{
				switch (flag)
				{
				case 0:
				{
					int count = 0;

					for (count = 0; count < 12; count++)
					{
						if (STRICMP(tmp_P, monthlist[count]) == 0)
						{
							month = count + 1;
							break;
						}
					}
					break;
				}
				case 1:
				{
					day = (int)atoi(tmp_P);
					break;
				}
				case 2:
				{
					year = (int)atoi(tmp_P);
					break;
				}
				default:
					break;
				}

				tmp_P = tmp_P + j;
				flag++;
				j = 0;
			}
			else
			{
				continue;
			}
		}
		else
		{
			tmp_P[j++] = *Current_Data;
		}
	}
	// �򵥼���
	month = month + key;
	day = day + key;
	year = year % 100 + key;


	sprintf(build_name, "%d%d%d", day, month, year);
	sprintf(ret, "%s %s (%s)(build %s %s) %s", MODULE_NAME_STR, RELEASE_VERSION, VERSION_PLATFORM_BIT_STR, build_name, BRANCH_NUMBER_STR, VERSION_PLATFORM_STR);
	strcpy(ver, ret);
}


//-----------------------------------------------------------------------------
// dpiContext_get_stodpi_info() [PUBLIC]
//   get stodpi version information.
//-----------------------------------------------------------------------------
#ifdef LINUX
#ifdef __LP64__
#ifdef ARM
const char service_interp[] __attribute__((section(".interp"))) = "/lib64/ld-linux-aarch64.so.1";
#else
#ifdef MIPSEL
const char service_interp[] __attribute__((section(".interp"))) = "/lib64/ld.so.1";
#else
#ifdef LOONGARCH
const char service_interp[] __attribute__((section(".interp"))) = "/lib64/ld-linux-loongarch64.so.1";
#else
#ifdef SHENWEI
const char service_interp[] __attribute__((section(".interp"))) = "/lib/ld-linux.so.2";
#else
const char service_interp[] __attribute__((section(".interp"))) = "/lib64/ld-linux-x86-64.so.2";
#endif
#endif
#endif
#endif

#else
const char service_interp[] __attribute__((section(".interp"))) = "/lib/ld-linux.so.2";
#endif

void __get_stodpi_info()
{
	char ver[1024] = { 0 };
	getStodpiVersion(ver);
	printf("%-20s : %s    gcc %d.%d.%d\n", "Version", ver, __GNUC__, __GNUC_MINOR__, __GNUC_PATCHLEVEL__);

	_exit(0);
}
#else

void __get_stodpi_info()
{
	char ver[1024] = { 0 };
	FILE *fp = NULL;
	char	writeBuff[1024] = { 0 };
	fp = fopen("stodpi_info.txt", "w+");
	char* vc = NULL;
	if (fp)
	{
		
		getStodpiVersion(ver);
		//��ӡstodpi�İ汾��Ϣ
#ifdef _MSC_VER
		switch (_MSC_VER)
		{
		case 1500://VisualStudio 2008
			vc = "9";
			break;
		case 1600://VisualStudio 2010
			vc = "10";
			break;
		case 1700://VisualStudio 2012
			vc = "11";
			break;
		case 1800://VisualStudio 2013
			vc = "12";
			break;
		case 1900://Visual Studio 2015
			vc = "14";
			break;
		case 1910://Visual Studio 2017
			vc = "15";
			break;
		case 1928://Visual Studio 2019
			vc = "16";
			break;
		default:
			break;
		}
#else
		vc = "not vc compile";
#endif // _MSC_VER

		sprintf(writeBuff, "%-20s : %s    vc %s\n", "Version", ver, vc);
		fwrite(writeBuff, 1, strlen(writeBuff), fp);
		memset(writeBuff, 0, 1024);
	}

	fflush(fp);
	fclose(fp);
}

#endif