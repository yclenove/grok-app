"""All regular headers in ANGLE's eight copied API trees and Khronos tree.

Fixed objects come from the original sealed ANGLE tree. The collector also
checks the four actual checkout manifests; headers are never executed.
"""


API_DIRECTORIES = ("CL", "EGL", "GLES", "GLES2", "GLES3", "GLX", "KHR", "WGL")
ANGLE_HEADER_PINS = {
    "include/CL/cl.h": "792e20cc8a1d6c405d29329dc0a737f658827094",
    "include/CL/cl_d3d10.h": "6b56c775bf8a53bbabb7cf6c6afe4b1de81fe3ae",
    "include/CL/cl_d3d11.h": "384c8f428fc5b8560fc9a3dfa89e9b85dc80420d",
    "include/CL/cl_dx9_media_sharing.h": "b079379d0a726970ffc4cdb5f857a029b110b783",
    "include/CL/cl_dx9_media_sharing_intel.h": "f6518d7f632227ee492e0a28bde5262727ef96cf",
    "include/CL/cl_egl.h": "68aefec7619fb42928e2afbf313f0f4b9d324c38",
    "include/CL/cl_ext.h": "ffefe075cb62b1e19c05ea8f53577fa5e97064a0",
    "include/CL/cl_ext_intel.h": "a7ae87a3400ffb0d3f3411dc0f4a3a330fcccf70",
    "include/CL/cl_function_types.h": "124f53ba7ec15244d11f3a49945cfe19d9658287",
    "include/CL/cl_gl.h": "552560f71c4e4f107c546131306c20bb0479ed85",
    "include/CL/cl_gl_ext.h": "b5da13eb67050daeb1e79044fa1f08d5ab8026ef",
    "include/CL/cl_half.h": "ecc42233220284c51bdd8fc01d446ec77232ac50",
    "include/CL/cl_icd.h": "435ad4832c6f753a9306aa013cf6995780f36421",
    "include/CL/cl_layer.h": "245f7b53295a285c25d566a218aecbb0a224ad01",
    "include/CL/cl_platform.h": "5f92d6faad4e68b66c75514320e245fe6b74ebd6",
    "include/CL/cl_va_api_media_sharing_intel.h": "9fb8863f24d9f3ad179bd839af70be1e084c749c",
    "include/CL/cl_version.h": "3844938d548b6751a68ec015c08dea6a9935860f",
    "include/CL/opencl.h": "ef8dd1e032ad280ebabee811615635650260f9c4",
    "include/EGL/egl.h": "51c1b2d4eb0393a5568a78c98c106f5d5b4fb88e",
    "include/EGL/eglext.h": "f8d2ccec45d42d5d1d19ecbd53109193895de915",
    "include/EGL/eglext_angle.h": "2daad197724c08588957a0d0eca71b85eb7b3e9a",
    "include/EGL/eglplatform.h": "101fd74d221a149be3756010a80366b071bd29e9",
    "include/GLES/egl.h": "86f644c923ba17496ce60168dfcaa9321daf154e",
    "include/GLES/gl.h": "969cc0c34fcab211cf0dd036f76b851b7df6a37c",
    "include/GLES/glext.h": "d8933e37d56b885ed2d571975191ba69394b7706",
    "include/GLES/glplatform.h": "16060a9a73ba32a7eedf44c4ccdd512d8fafc637",
    "include/GLES2/gl2.h": "fdc46fc951e560c5999f0d2e9e8a8a6c9cb5255d",
    "include/GLES2/gl2ext.h": "581dbada1ef73af9e1af496e4817867760397aa9",
    "include/GLES2/gl2ext_angle.h": "6cdfc7dc551dae5d1fbf800816635550897916f3",
    "include/GLES2/gl2platform.h": "5bcce6d89e0992419294ce95a269427a2811e2f7",
    "include/GLES3/gl3.h": "388e880fb66768924f524bbb1f4fc6ea7ce42d7c",
    "include/GLES3/gl31.h": "8ed7129c516bb779ae6caa9a0edcc89c8bf1bf4b",
    "include/GLES3/gl32.h": "56e92144162d7d43b9f78ac35c0ef96d3fc456ac",
    "include/GLES3/gl3platform.h": "8699212de64d4050ed97edd548e6c7cee6f56c0e",
    "include/GLX/glxext.h": "b918317fb9741bd6f998f5a58849dafb71a0234d",
    "include/KHR/khrplatform.h": "dd22d9270188f2878f5dd47806497b16b34db0fe",
    "include/WGL/wgl.h": "0bae52bddae46f5a4dfd2bf37a8fe7bb462af09c",
    "src/third_party/khronos/GL/wglext.h": "daba41091c4422269affb7583b88f22139142650",
}
