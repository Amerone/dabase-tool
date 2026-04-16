#-----------------------------------------
# OS: target operation system, currently support: Linux, Aix, Solaris, Hpunix
#-----------------------------------------
OS = Linux

#-----------------------------------------
# ARCH: target architecture, currently support: MIPSEL SHENWEI FEITENG ARM LOONGARCH DEFAULT
#-----------------------------------------
ARCH = DEFAULT

#-----------------------------------------
# TYPE: compile type, currently support: debug, release
#-----------------------------------------
TYPE = release

#-----------------------------------------
# include dirs, source dirs and library dirs 
#-----------------------------------------
INC_DIR = include
SRC_DIR = .
LIB_DIR = lib
LIB_FILE = linux64

#-----------------------------------------
# odpi library properties
#-----------------------------------------
ODPI_NAME = libstodpi

#-----------------------------------------
# 32bit or 64bit operating system
#-----------------------------------------
BIT = 64

#-----------------------------------------
#release version
#-----------------------------------------
RELEASE_VERSION=4.0.0

#-----------------------------------------
# CC options: gcc or cc
#-----------------------------------------
# CC = cc
	CC = gcc
# CC = xlc_r

ifeq (cc, $(CC))
  _PIC = -KPIC
  _MODE = -G 
  _OTHER = -w -xCC -Xa
endif

ifeq (gcc, $(CC))
	ifeq ($(ARCH), SHENWEI)
  	_PIC = -fpic -O2 
	else
	  _PIC = -fpic -fvisibility=hidden -O2
	endif

  ifeq ($(BIT), 64)
	_MODE = -shared -m64	
  endif
  ifeq ($(BIT), 32)
    _MODE = -shared -m32
  endif
endif


ifeq (Linux, $(OS))
  ifeq ($(ARCH), DEFAULT)
    ifeq ($(BIT), 64)
	    LIB_FILE = linux64
  	    CPPFLAGS := $(_PIC) -I$(INC_DIR) -I$(SRC_DIR) -D_GNU_SOURCE -DLINUX -DOSC_64BIT_ARCH -m64 
  	    _MODE = -shared -m64
  	    _LIB_NAME = -Wl,-e,__get_stodpi_info,-soname,$(ODPI_NAME).so
    endif
    ifeq ($(BIT), 32)
		LIB_FILE = linux32
	    	CPPFLAGS := $(_PIC) -I$(INC_DIR) -I$(SRC_DIR) -DLINUX -D_GNU_SOURCE -m32
		 		_MODE = -shared -m32
		 		_LIB_NAME = -Wl,-e,__get_stodpi_info,-soname,$(ODPI_NAME).so
    endif
    ifeq ($(TYPE), debug)
      CFLAGS +=  -g -ggdb 
    else
      ifeq ($(TYPE), release)
        CFLAGS += -gstabs -DNDEBUG -O2 -fomit-frame-pointer -fno-strict-aliasing
      endif
    endif
  endif

  ifeq ($(ARCH), ARM)    
      CPPFLAGS := $(_PIC) -I$(INC_DIR) -I$(SRC_DIR) -DLINUX 
      ifeq ($(BIT), 64)
  	    LIB_FILE = arm64
    	    CPPFLAGS +=  -DARM
    	    _MODE = -shared
    	    _LIB_NAME = -Wl,-e,__get_stodpi_info,-soname,$(ODPI_NAME).so
      endif
  
    ifeq ($(TYPE), debug)
      CPPFLAGS +=  -g -ggdb 
    else
      ifeq ($(TYPE), release)
        CPPFLAGS += -DNDEBUG -fomit-frame-pointer -fno-strict-aliasing
      endif
    endif
  endif
  
  ifeq ($(ARCH), MIPSEL)
      CPPFLAGS := $(_PIC) -I$(INC_DIR) -I$(SRC_DIR) -DLINUX 
      ifeq ($(BIT), 64)
  	    LIB_FILE = mipsel64
    	    CPPFLAGS +=  -DMIPSEL -mabi=64 
    	    _MODE = -shared -mabi=64
    	    _LIB_NAME = -Wl,-e,__get_stodpi_info,-soname,$(ODPI_NAME).so
      endif
      ifeq ($(BIT), 32)
  		LIB_FILE = mipsel32
  	    	CPPFLAGS += -DMIPSEL -mabi=32
  		_MODE = -shared -mabi=32
  		_LIB_NAME = -Wl,-e,__get_stodpi_info,-soname,$(ODPI_NAME).so
      endif
    ifeq ($(TYPE), debug)
      CPPFLAGS +=  -g -ggdb 
    else
      ifeq ($(TYPE), release)
        CPPFLAGS += -gstabs -DNDEBUG -fomit-frame-pointer -fno-strict-aliasing
      endif
    endif
  endif

  ifeq ($(ARCH), SHENWEI)
    CPPFLAGS := $(_PIC) -I$(INC_DIR) -I$(SRC_DIR) -DLINUX -DSHENWEI
    ifeq ($(BIT), 64)
    LIB_FILE = shenwei64
        _MODE = -shared
        _LIB_NAME = -Wl,-e,__get_stodpi_info,-soname,$(ODPI_NAME).so
    endif
  endif

endif

#OS_64_BIT mark a 64-bit OS
ifeq ($(BIT), 64)
	CPPFLAGS += -DOS_64_BIT	
endif

CPPFLAGS += -DRELEASE_VERSION=\"${RELEASE_VERSION}\"

OBJS = $(patsubst %.c,%.o,$(wildcard src/*.c))

all : $(OBJS)
	mkdir -p $(OS)$(BIT)

ifeq (Linux, $(OS))
	$(CC) $(CFLAGS) $(_PIC) $(_MODE) $(OBJS) -lstdc++ -ldl $(_LIB_NAME) -o $(OS)$(BIT)/$(ODPI_NAME).so
endif
clean : 
	rm -f $(OBJS)
	rm -f $(OS)$(BIT)/$(ODPI_NAME).so

