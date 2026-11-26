#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <IOSurface/IOSurface.h>
#include <stdio.h>
#include <string.h>

@interface IPPMetalTarget : NSObject
@property(nonatomic, strong) id<MTLTexture> color;
@property(nonatomic, strong) id<MTLTexture> depth;
@property(nonatomic, strong) id<MTLTexture> owner;
@end
@implementation IPPMetalTarget
@end

static const size_t CanvasColumnBytes = 4 * 4 * sizeof(float);

@interface IPPMetalColumnMap : NSObject
@property(nonatomic, strong) NSData *mappings;
@property(nonatomic, strong) id<MTLBuffer> buffer;
@property(nonatomic) uint32_t width;
@property(nonatomic) uint32_t height;
@property(atomic) bool ready;
@end
@implementation IPPMetalColumnMap
@end

@interface IPPMetalDevice : NSObject
@property(nonatomic, strong) id<MTLDevice> gpu;
@property(nonatomic, strong) id<MTLCommandQueue> queue;
@property(nonatomic, strong) NSMutableArray<id<MTLCommandBuffer>> *submitted;
@property(nonatomic, strong) id<MTLRenderPipelineState> meshRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> meshImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> alphaRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> alphaImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> additiveRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> additiveImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasClear;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasLayers;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasAffineRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasAffineImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasPathRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasPathImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasShapeRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasShapeImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasCylinderPathRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasCylinderPathImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasColumnRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasColumnImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasColumnPathRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasColumnPathImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasTextureRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> canvasTextureImage;
@property(nonatomic, strong) id<MTLDepthStencilState> depthState;
@property(nonatomic, strong) id<MTLDepthStencilState> canvasDepth;
@property(nonatomic, strong) id<MTLDepthStencilState> canvasOverlay;
@property(nonatomic, strong) id<MTLTexture> white;
@property(nonatomic, strong) id<MTLTexture> emptyDepth;
@property(nonatomic, strong) id<MTLTexture> emptyOwner;
@property(nonatomic, strong) id<MTLRenderPipelineState> shadowPipeline;
@property(nonatomic, strong) id<MTLComputePipelineState> webImagePipeline;
@property(nonatomic, strong) id<MTLComputePipelineState> canvasColumnsPipeline;
@property(nonatomic, strong) id<MTLBuffer> emptyColumns;
@property(nonatomic, strong) NSMutableArray<IPPMetalColumnMap *> *columnCache;
@property(nonatomic, strong) NSMutableData *previousColumnMappings;
@property(nonatomic) uint32_t previousColumnWidth;
@property(nonatomic) uint32_t previousColumnHeight;
@property(nonatomic, strong) IPPMetalTarget *root;
@end
@implementation IPPMetalDevice
@end

@interface IPPMetalCanvasProgram : NSObject
@property(nonatomic, strong) id<MTLRenderPipelineState> root;
@property(nonatomic, strong) id<MTLRenderPipelineState> image;
@property(nonatomic, strong) id<MTLRenderPipelineState> shapeRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> shapeImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> columnRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> columnImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> layers;
@property(nonatomic, strong) id<MTLRenderPipelineState> affineRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> affineImage;
@end
@implementation IPPMetalCanvasProgram
@end

@interface IPPMetalMaterialProgram : NSObject
@property(nonatomic, strong) id<MTLRenderPipelineState> root;
@property(nonatomic, strong) id<MTLRenderPipelineState> image;
@property(nonatomic, strong) id<MTLRenderPipelineState> alphaRoot;
@property(nonatomic, strong) id<MTLRenderPipelineState> alphaImage;
@property(nonatomic, strong) id<MTLRenderPipelineState> shadow;
@end
@implementation IPPMetalMaterialProgram
@end

typedef struct {
    float clip[4];
    float local[2];
    float world[4];
    float right[3];
    float down[3];
    float normal[3];
    uint32_t slot;
} IPPMetalCanvasVertex;

_Static_assert(sizeof(IPPMetalCanvasVertex) == 80, "Canvas vertex layout");

@interface IPPMetalCanvasBatch : NSObject
@property(nonatomic, strong) NSMutableData *vertices;
@property(nonatomic, strong) NSMutableData *paints;
@property(nonatomic, strong) NSMutableData *bindings;
@property(nonatomic, strong) NSMutableData *mapping;
@property(nonatomic, strong) NSMutableData *intersections;
@property(nonatomic, strong) NSMutableData *edges;
@property(nonatomic, strong) NSMutableData *parameters;
@property(nonatomic, strong) id<MTLTexture> color;
@property(nonatomic, strong) id<MTLTexture> mask;
@property(nonatomic, strong) id<MTLBuffer> path;
@property(nonatomic, strong) id<MTLBuffer> retainedData;
@property(nonatomic, strong) IPPMetalCanvasProgram *program;
@property(nonatomic) uint32_t count;
@property(nonatomic) bool depthTest;
@property(nonatomic) bool analytic;
@property(nonatomic) uint8_t specialization;
- (void)reset;
@end

@implementation IPPMetalCanvasBatch
- (instancetype)init {
    if (!(self = [super init])) return nil;
    _vertices = [NSMutableData dataWithCapacity:4096];
    _paints = [NSMutableData dataWithCapacity:4096];
    _bindings = [NSMutableData dataWithCapacity:1024];
    _mapping = [NSMutableData dataWithCapacity:224];
    _intersections = [NSMutableData dataWithCapacity:4096];
    _edges = [NSMutableData dataWithCapacity:256];
    _parameters = [NSMutableData dataWithCapacity:256];
    return self;
}

- (void)reset {
    _vertices.length = 0;
    _paints.length = 0;
    _bindings.length = 0;
    _mapping.length = 0;
    _intersections.length = 0;
    _edges.length = 0;
    _parameters.length = 0;
    _color = nil;
    _mask = nil;
    _path = nil;
    _program = nil;
    _count = 0;
}
@end

@interface IPPMetalFrame : NSObject
@property(nonatomic, strong) id<MTLCommandBuffer> commands;
@property(nonatomic, strong) id<MTLRenderCommandEncoder> encoder;
@property(nonatomic, strong) id<MTLTexture> color;
@property(nonatomic, strong) IPPMetalDevice *device;
@property(nonatomic, strong) IPPMetalTarget *target;
@property(nonatomic) uint8_t drawState;
@property(nonatomic) bool depthOnly;
@property(nonatomic) bool encodingFailed;
@property(nonatomic) uint32_t mergedCanvasDraws;
@property(nonatomic, strong) IPPMetalCanvasBatch *canvasBatch;
@property(nonatomic, strong) NSMutableArray<IPPMetalCanvasBatch *> *canvasRecording;
@property(nonatomic) size_t canvasRecordedBytes;
@property(nonatomic, strong) id<MTLBuffer> canvasUpload;
@property(nonatomic) size_t canvasUploadOffset;
@property(nonatomic, strong) id<MTLBuffer> canvasColumns;
@property(nonatomic, strong) IPPMetalCanvasProgram *paint;
@property(nonatomic, strong) IPPMetalMaterialProgram *material;
@end
@implementation IPPMetalFrame
- (void)dealloc {
    if (_encoder) [_encoder endEncoding];
}
@end

static bool flush_canvas(IPPMetalFrame *frame);

static void describe(char *message, size_t capacity, NSString *reason) {
    snprintf(message, capacity, "%s", reason.UTF8String ?: "Metal operation failed");
}

static id<MTLTexture> texture(id<MTLDevice> gpu, MTLPixelFormat format,
                              uint32_t width, uint32_t height, MTLStorageMode storage,
                              MTLTextureUsage usage) {
    MTLTextureDescriptor *desc =
        [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:format
                                                          width:width height:height mipmapped:NO];
    desc.storageMode = storage;
    desc.usage = usage;
    return [gpu newTextureWithDescriptor:desc];
}

enum {
    IPPMetalCanvasColumnShape = 8,
    IPPMetalCanvasColumnPath,
    IPPMetalCanvasCylinderShape,
    IPPMetalCanvasCylinderPath,
};

static id<MTLRenderPipelineState> pipeline(IPPMetalDevice *device, id<MTLLibrary> library,
                                           MTLPixelFormat format, uint8_t mode, bool custom, NSError **error) {
    MTLRenderPipelineDescriptor *desc = [MTLRenderPipelineDescriptor new];
    bool shapesOnly = mode == IPPMetalCanvasColumnShape || mode == IPPMetalCanvasCylinderShape;
    bool columns = mode == IPPMetalCanvasColumnShape || mode == IPPMetalCanvasColumnPath;
    bool cylinder = mode == IPPMetalCanvasCylinderShape || mode == IPPMetalCanvasCylinderPath;
    if (shapesOnly) mode = 3;
    if (mode == IPPMetalCanvasColumnPath || mode == IPPMetalCanvasCylinderPath) mode = 6;
    bool canvas = mode >= 3;
    desc.label = canvas ? @"IPP Canvas" : @"IPP meshes";
    NSString *vertex = custom ? @"custom_vertex" : @"mesh_vertex";
    if (canvas) vertex = mode == 3 || mode == 6 ? @"canvas_intersection_vertex" : @"canvas_vertex";
    desc.vertexFunction = [library newFunctionWithName:vertex];
    NSString *fragment = canvas ? @"canvas_fragment" : (custom ? @"custom_fragment" : @"mesh_fragment");
    if (mode == 4) {
        fragment = @"canvas_layer_fragment";
        desc.colorAttachments[1].pixelFormat = MTLPixelFormatR32Uint;
    }
    if (mode == 5) fragment = @"canvas_affine_fragment";
    if (mode == 6) fragment = @"canvas_path_fragment";
    if (mode == 7) fragment = @"canvas_image_fragment";
    if (canvas) {
        MTLFunctionConstantValues *constants = [MTLFunctionConstantValues new];
        [constants setConstantValue:&columns type:MTLDataTypeBool atIndex:0];
        [constants setConstantValue:&shapesOnly type:MTLDataTypeBool atIndex:1];
        [constants setConstantValue:&cylinder type:MTLDataTypeBool atIndex:2];
        desc.fragmentFunction = [library newFunctionWithName:fragment constantValues:constants error:error];
        if (!desc.fragmentFunction) return nil;
    } else {
        desc.fragmentFunction = [library newFunctionWithName:fragment];
    }
    MTLRenderPipelineColorAttachmentDescriptor *color = desc.colorAttachments[0];
    color.pixelFormat = format;
    color.blendingEnabled = mode != 0;
    color.sourceRGBBlendFactor = canvas ? MTLBlendFactorOne : MTLBlendFactorSourceAlpha;
    color.sourceAlphaBlendFactor = MTLBlendFactorOne;
    color.destinationRGBBlendFactor = mode == 2 ? MTLBlendFactorOne : MTLBlendFactorOneMinusSourceAlpha;
    color.destinationAlphaBlendFactor = mode == 2 ? MTLBlendFactorOne : MTLBlendFactorOneMinusSourceAlpha;
    desc.depthAttachmentPixelFormat = MTLPixelFormatDepth32Float;
    return [device.gpu newRenderPipelineStateWithDescriptor:desc error:error];
}

static IPPMetalTarget *make_target(IPPMetalDevice *device, MTLPixelFormat format,
                                   uint32_t width, uint32_t height, bool sampled, bool layered) {
    IPPMetalTarget *target = [IPPMetalTarget new];
    MTLTextureUsage usage = MTLTextureUsageRenderTarget | (sampled ? MTLTextureUsageShaderRead : 0);
    target.color = texture(device.gpu, format, width, height, MTLStorageModePrivate, usage);
    MTLStorageMode depthStorage = [device.gpu supportsFamily:MTLGPUFamilyApple1]
        ? MTLStorageModeMemoryless : MTLStorageModePrivate;
    target.depth = texture(device.gpu, MTLPixelFormatDepth32Float, width, height,
                           depthStorage, MTLTextureUsageRenderTarget);
    if (layered) {
        target.owner = texture(device.gpu, MTLPixelFormatR32Uint, width, height,
                               MTLStorageModePrivate, usage);
    }
    return target.color && target.depth && (!layered || target.owner) ? target : nil;
}

static id<MTLLibrary> compile(id<MTLDevice> gpu, const char *source, size_t domainLightLimit,
                              NSError **error) {
    MTLCompileOptions *options = [MTLCompileOptions new];
    // Allow fused arithmetic while preserving the non-finite checks used by
    // analytic coverage and authored shaders.
    if (@available(macOS 15.0, *)) options.mathMode = MTLMathModeRelaxed;
    options.preprocessorMacros = @{@"IPP_MAX_DOMAIN_LIGHTS": @(domainLightLimit)};
    return [gpu newLibraryWithSource:[NSString stringWithUTF8String:source] options:options error:error];
}

void *ipp_metal_canvas_program(void *handle, const char *source, size_t domainLightLimit,
                              char *message, size_t capacity) {
    @autoreleasepool {
        IPPMetalDevice *device = (__bridge IPPMetalDevice *)handle;
        NSError *error = nil;
        id<MTLLibrary> library = compile(device.gpu, source, domainLightLimit, &error);
        IPPMetalCanvasProgram *program = [IPPMetalCanvasProgram new];
        if (library)
            program.root = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, 3, false, &error);
        if (program.root)
            program.image = pipeline(device, library, MTLPixelFormatRGBA16Float, 3, false, &error);
        if (program.image)
            program.layers = pipeline(device, library, MTLPixelFormatRGBA16Float, 4, false, &error);
        if (program.layers)
            program.affineRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, 5, false, &error);
        if (program.affineRoot)
            program.affineImage = pipeline(device, library, MTLPixelFormatRGBA16Float, 5, false, &error);
        if (program.affineImage)
            program.columnRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, IPPMetalCanvasColumnShape, false, &error);
        if (program.columnRoot)
            program.columnImage = pipeline(device, library, MTLPixelFormatRGBA16Float, IPPMetalCanvasColumnShape, false, &error);
        if (program.columnImage)
            program.shapeRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, IPPMetalCanvasCylinderShape, false, &error);
        if (program.shapeRoot)
            program.shapeImage = pipeline(device, library, MTLPixelFormatRGBA16Float, IPPMetalCanvasCylinderShape, false, &error);
        if (!program.shapeImage) {
            describe(message, capacity, error.localizedDescription);
            return NULL;
        }
        return (__bridge_retained void *)program;
    }
}

void *ipp_metal_material_program(void *handle, const char *source, const char *shadowSource,
                                 size_t domainLightLimit, char *message, size_t capacity) {
    @autoreleasepool {
        IPPMetalDevice *device = (__bridge IPPMetalDevice *)handle;
        NSError *error = nil;
        id<MTLLibrary> library = compile(device.gpu, source, domainLightLimit, &error);
        IPPMetalMaterialProgram *program = [IPPMetalMaterialProgram new];
        if (library)
            program.root = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, 0, true, &error);
        if (program.root)
            program.image = pipeline(device, library, MTLPixelFormatRGBA16Float, 0, true, &error);
        if (program.image)
            program.alphaRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, 1, true, &error);
        if (program.alphaRoot)
            program.alphaImage = pipeline(device, library, MTLPixelFormatRGBA16Float, 1, true, &error);
        if (program.alphaImage && shadowSource) {
            id<MTLLibrary> shadow = compile(device.gpu, shadowSource, domainLightLimit, &error);
            if (shadow) {
                MTLRenderPipelineDescriptor *desc = [MTLRenderPipelineDescriptor new];
                desc.label = @"IPP custom shadow";
                desc.vertexFunction = [shadow newFunctionWithName:@"custom_vertex"];
                desc.fragmentFunction = [shadow newFunctionWithName:@"custom_fragment"];
                desc.depthAttachmentPixelFormat = MTLPixelFormatDepth32Float;
                program.shadow = [device.gpu newRenderPipelineStateWithDescriptor:desc error:&error];
            }
        }
        if (!program.alphaImage || (shadowSource && !program.shadow)) {
            describe(message, capacity, error.localizedDescription);
            return NULL;
        }
        return (__bridge_retained void *)program;
    }
}

void *ipp_metal_device(const char *source, size_t domainLightLimit, char *message, size_t capacity) {
    @autoreleasepool {
        IPPMetalDevice *device = [IPPMetalDevice new];
        device.gpu = MTLCreateSystemDefaultDevice();
        device.queue = [device.gpu newCommandQueue];
        device.submitted = [NSMutableArray arrayWithCapacity:3];
        if (!device.queue) {
            describe(message, capacity, @"No Metal device or command queue");
            return NULL;
        }
        NSError *error = nil;
        id<MTLLibrary> library = compile(device.gpu, source, domainLightLimit, &error);
        if (!library) {
            describe(message, capacity, error.localizedDescription);
            return NULL;
        }
        device.meshRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, 0, false, &error);
        if (device.meshRoot)
            device.meshImage = pipeline(device, library, MTLPixelFormatRGBA16Float, 0, false, &error);
        if (device.meshImage)
            device.canvasRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, 3, false, &error);
        if (device.canvasRoot)
            device.canvasImage = pipeline(device, library, MTLPixelFormatRGBA16Float, 3, false, &error);
        if (device.canvasImage)
            device.canvasLayers = pipeline(device, library, MTLPixelFormatRGBA16Float, 4, false, &error);
        if (device.canvasLayers)
            device.canvasAffineRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, 5, false, &error);
        if (device.canvasAffineRoot)
            device.canvasAffineImage = pipeline(device, library, MTLPixelFormatRGBA16Float, 5, false, &error);
        if (device.canvasAffineImage)
            device.canvasPathRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, 6, false, &error);
        if (device.canvasPathRoot)
            device.canvasPathImage = pipeline(device, library, MTLPixelFormatRGBA16Float, 6, false, &error);
        if (device.canvasPathImage)
            device.canvasColumnRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, IPPMetalCanvasColumnShape, false, &error);
        if (device.canvasColumnRoot)
            device.canvasColumnImage = pipeline(device, library, MTLPixelFormatRGBA16Float, IPPMetalCanvasColumnShape, false, &error);
        if (device.canvasColumnImage)
            device.canvasColumnPathRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, IPPMetalCanvasColumnPath, false, &error);
        if (device.canvasColumnPathRoot)
            device.canvasColumnPathImage = pipeline(device, library, MTLPixelFormatRGBA16Float, IPPMetalCanvasColumnPath, false, &error);
        if (device.canvasColumnPathImage)
            device.canvasShapeRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, IPPMetalCanvasCylinderShape, false, &error);
        if (device.canvasShapeRoot)
            device.canvasShapeImage = pipeline(device, library, MTLPixelFormatRGBA16Float, IPPMetalCanvasCylinderShape, false, &error);
        if (device.canvasShapeImage)
            device.canvasCylinderPathRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, IPPMetalCanvasCylinderPath, false, &error);
        if (device.canvasCylinderPathRoot)
            device.canvasCylinderPathImage = pipeline(device, library, MTLPixelFormatRGBA16Float, IPPMetalCanvasCylinderPath, false, &error);
        if (device.canvasCylinderPathImage)
            device.canvasTextureRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, 7, false, &error);
        if (device.canvasTextureRoot)
            device.canvasTextureImage = pipeline(device, library, MTLPixelFormatRGBA16Float, 7, false, &error);
        if (device.canvasTextureImage)
            device.alphaRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, 1, false, &error);
        if (device.alphaRoot)
            device.alphaImage = pipeline(device, library, MTLPixelFormatRGBA16Float, 1, false, &error);
        if (device.alphaImage)
            device.additiveRoot = pipeline(device, library, MTLPixelFormatRGBA8Unorm_sRGB, 2, false, &error);
        if (device.additiveRoot)
            device.additiveImage = pipeline(device, library, MTLPixelFormatRGBA16Float, 2, false, &error);
        if (!device.additiveImage) {
            describe(message, capacity, error.localizedDescription);
            return NULL;
        }

        MTLRenderPipelineDescriptor *clear = [MTLRenderPipelineDescriptor new];
        clear.label = @"IPP Canvas damage clear";
        clear.vertexFunction = [library newFunctionWithName:@"canvas_clear_vertex"];
        clear.fragmentFunction = [library newFunctionWithName:@"canvas_clear_fragment"];
        clear.colorAttachments[0].pixelFormat = MTLPixelFormatRGBA16Float;
        clear.depthAttachmentPixelFormat = MTLPixelFormatDepth32Float;
        device.canvasClear = [device.gpu newRenderPipelineStateWithDescriptor:clear error:&error];
        if (!device.canvasClear) {
            describe(message, capacity, error.localizedDescription);
            return NULL;
        }

        MTLRenderPipelineDescriptor *shadow = [MTLRenderPipelineDescriptor new];
        shadow.label = @"IPP spotlight depth";
        shadow.vertexFunction = [library newFunctionWithName:@"mesh_vertex"];
        shadow.depthAttachmentPixelFormat = MTLPixelFormatDepth32Float;
        device.shadowPipeline = [device.gpu newRenderPipelineStateWithDescriptor:shadow error:&error];
        if (!device.shadowPipeline) {
            describe(message, capacity, error.localizedDescription);
            return NULL;
        }

        id<MTLFunction> columns = [library newFunctionWithName:@"canvas_columns"];
        if (columns) device.canvasColumnsPipeline = [device.gpu newComputePipelineStateWithFunction:columns error:NULL];
        device.columnCache = [NSMutableArray arrayWithCapacity:4];
        device.previousColumnMappings = [NSMutableData data];
        device.emptyColumns = [device.gpu newBufferWithLength:CanvasColumnBytes options:MTLResourceStorageModeShared];
        if (!device.emptyColumns) {
            describe(message, capacity, @"Could not allocate Canvas column binding");
            return NULL;
        }

        id<MTLFunction> import = [library newFunctionWithName:@"import_web_image"];
        device.webImagePipeline = [device.gpu newComputePipelineStateWithFunction:import error:&error];
        if (!device.webImagePipeline) {
            describe(message, capacity, error.localizedDescription);
            return NULL;
        }

        MTLDepthStencilDescriptor *depth = [MTLDepthStencilDescriptor new];
        depth.depthCompareFunction = MTLCompareFunctionLess;
        depth.depthWriteEnabled = YES;
        device.depthState = [device.gpu newDepthStencilStateWithDescriptor:depth];
        depth.depthWriteEnabled = NO;
        depth.depthCompareFunction = MTLCompareFunctionLessEqual;
        device.canvasDepth = [device.gpu newDepthStencilStateWithDescriptor:depth];
        depth.depthCompareFunction = MTLCompareFunctionAlways;
        device.canvasOverlay = [device.gpu newDepthStencilStateWithDescriptor:depth];
        device.white = texture(device.gpu, MTLPixelFormatRGBA8Unorm_sRGB, 1, 1,
                               MTLStorageModeShared, MTLTextureUsageShaderRead);
        device.emptyOwner = texture(device.gpu, MTLPixelFormatR32Uint, 1, 1,
                                    MTLStorageModePrivate, MTLTextureUsageShaderRead);
        device.emptyDepth = texture(device.gpu, MTLPixelFormatDepth32Float, 1, 1,
                                    MTLStorageModePrivate, MTLTextureUsageShaderRead);
        if (!device.emptyDepth || !device.emptyOwner || !device.depthState || !device.canvasDepth || !device.canvasOverlay || !device.white) {
            describe(message, capacity, @"Could not allocate Metal depth state or fallback texture");
            return NULL;
        }
        const uint8_t pixel[4] = {255, 255, 255, 255};
        [device.white replaceRegion:MTLRegionMake2D(0, 0, 1, 1)
                        mipmapLevel:0 withBytes:pixel bytesPerRow:4];
        return (__bridge_retained void *)device;
    }
}

void ipp_metal_release(void *object) {
    @autoreleasepool {
        (void)CFBridgingRelease(object);
    }
}

void ipp_metal_name(void *handle, char *name, size_t capacity) {
    @autoreleasepool {
        IPPMetalDevice *device = (__bridge IPPMetalDevice *)handle;
        describe(name, capacity, device.gpu.name);
    }
}

bool ipp_metal_resize(void *handle, uint32_t width, uint32_t height) {
    @autoreleasepool {
        IPPMetalDevice *device = (__bridge IPPMetalDevice *)handle;
        if (device.root.color.width == width && device.root.color.height == height) return true;
        IPPMetalTarget *target = make_target(device, MTLPixelFormatRGBA8Unorm_sRGB, width, height, false, false);
        if (!target) return false;
        device.root = target;
        return true;
    }
}

void *ipp_metal_target(void *handle, uint32_t width, uint32_t height, bool layers) {
    @autoreleasepool {
        IPPMetalDevice *device = (__bridge IPPMetalDevice *)handle;
        return (__bridge_retained void *)make_target(device, MTLPixelFormatRGBA16Float, width, height, true, layers);
    }
}

void *ipp_metal_buffer(void *handle, const void *bytes, size_t length) {
    @autoreleasepool {
        IPPMetalDevice *device = (__bridge IPPMetalDevice *)handle;
        id<MTLBuffer> buffer = bytes
            ? [device.gpu newBufferWithBytes:bytes length:length options:MTLResourceStorageModeShared]
            : [device.gpu newBufferWithLength:length options:MTLResourceStorageModeShared];
        return (__bridge_retained void *)buffer;
    }
}

bool ipp_metal_write_buffer(void *handle, size_t offset, const void *bytes, size_t length) {
    id<MTLBuffer> buffer = (__bridge id<MTLBuffer>)handle;
    if (offset > buffer.length || length > buffer.length - offset) return false;
    memcpy((uint8_t *)buffer.contents + offset, bytes, length);
    return true;
}

void *ipp_metal_texture(void *handle, uint32_t width, uint32_t height, const void *pixels) {
    @autoreleasepool {
        IPPMetalDevice *device = (__bridge IPPMetalDevice *)handle;
        id<MTLTexture> result = texture(device.gpu, MTLPixelFormatRGBA8Unorm_sRGB, width, height,
                                        MTLStorageModeShared, MTLTextureUsageShaderRead);
        [result replaceRegion:MTLRegionMake2D(0, 0, width, height)
                  mipmapLevel:0 withBytes:pixels bytesPerRow:(size_t)width * 4];
        return (__bridge_retained void *)result;
    }
}

void *ipp_metal_web_image(void *handle, void *storage, uint32_t width, uint32_t height,
                          size_t budget, char *message, size_t capacity) {
    @autoreleasepool {
        IPPMetalDevice *device = (__bridge IPPMetalDevice *)handle;
        IOSurfaceRef surface = storage;
        if (!surface || IOSurfaceGetPlaneCount(surface) > 1
            || IOSurfaceGetWidth(surface) != width || IOSurfaceGetHeight(surface) != height
            || IOSurfaceGetPixelFormat(surface) != 'BGRA'
            || IOSurfaceGetAllocSize(surface) > budget) {
            describe(message, capacity, @"Invalid accelerated web-image extent, format or allocation");
            return NULL;
        }
        MTLTextureDescriptor *sourceDesc =
            [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                             width:width height:height mipmapped:NO];
        sourceDesc.storageMode = MTLStorageModeShared;
        sourceDesc.usage = MTLTextureUsageShaderRead;
        id<MTLTexture> source = [device.gpu newTextureWithDescriptor:sourceDesc iosurface:surface plane:0];
        id<MTLTexture> destination = texture(device.gpu, MTLPixelFormatRGBA16Float, width, height,
            MTLStorageModePrivate, MTLTextureUsageShaderWrite | MTLTextureUsageShaderRead);
        id<MTLCommandBuffer> commands = [device.queue commandBuffer];
        id<MTLComputeCommandEncoder> convert = [commands computeCommandEncoder];
        if (!source || !destination || !convert) {
            if (convert) [convert endEncoding];
            describe(message, capacity, @"Could not allocate the accelerated web-image import");
            return NULL;
        }
        commands.label = @"IPP IOSurface conversion";
        [convert setComputePipelineState:device.webImagePipeline];
        [convert setTexture:source atIndex:0];
        [convert setTexture:destination atIndex:1];
        const NSUInteger lane = device.webImagePipeline.threadExecutionWidth;
        const NSUInteger rows = MIN(8, device.webImagePipeline.maxTotalThreadsPerThreadgroup / lane);
        [convert dispatchThreads:MTLSizeMake(width, height, 1)
           threadsPerThreadgroup:MTLSizeMake(lane, rows, 1)];
        [convert endEncoding];
        [commands commit];
        // CEF owns the IOSurface bytes and can recycle them when the callback returns.
        [commands waitUntilCompleted];
        if (commands.status != MTLCommandBufferStatusCompleted) {
            describe(message, capacity, commands.error.localizedDescription);
            return NULL;
        }
        return (__bridge_retained void *)destination;
    }
}

void *ipp_metal_frame(void *handle) {
    @autoreleasepool {
        IPPMetalDevice *device = (__bridge IPPMetalDevice *)handle;
        if (!device.root) return NULL;
        IPPMetalFrame *frame = [IPPMetalFrame new];
        frame.device = device;
        frame.color = device.root.color;
        frame.commands = [device.queue commandBuffer];
        if (!frame.commands) return NULL;
        frame.commands.label = @"IPP root output";
        return (__bridge_retained void *)frame;
    }
}

void *ipp_metal_shadow_texture(void *handle, uint32_t size) {
    @autoreleasepool {
        IPPMetalDevice *device = (__bridge IPPMetalDevice *)handle;
        return (__bridge_retained void *)texture(device.gpu, MTLPixelFormatDepth32Float, size, size,
            MTLStorageModePrivate, MTLTextureUsageShaderRead | MTLTextureUsageRenderTarget);
    }
}

bool ipp_metal_shadow(void *handle, void *image) {
    @autoreleasepool {
        IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
        if (!flush_canvas(frame)) return false;
        [frame.encoder endEncoding];
        frame.encoder = nil;
        MTLRenderPassDescriptor *pass = [MTLRenderPassDescriptor renderPassDescriptor];
        pass.depthAttachment.texture = (__bridge id<MTLTexture>)image;
        pass.depthAttachment.loadAction = MTLLoadActionClear;
        pass.depthAttachment.storeAction = MTLStoreActionStore;
        pass.depthAttachment.clearDepth = 1;
        frame.encoder = [frame.commands renderCommandEncoderWithDescriptor:pass];
        if (!frame.encoder) return false;
        frame.depthOnly = true;
        frame.material = nil;
        [frame.encoder setRenderPipelineState:frame.device.shadowPipeline];
        [frame.encoder setDepthStencilState:frame.device.depthState];
        [frame.encoder setFrontFacingWinding:MTLWindingCounterClockwise];
        [frame.encoder setCullMode:MTLCullModeBack];
        return true;
    }
}

void ipp_metal_shadow_tile(void *handle, uint32_t x, uint32_t y, uint32_t size) {
    @autoreleasepool {
        IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
        [frame.encoder setViewport:(MTLViewport){x, y, size, size, 0, 1}];
        [frame.encoder setScissorRect:(MTLScissorRect){x, y, size, size}];
    }
}

bool ipp_metal_pass(void *handle, void *image, const float *background,
                    const void *lighting, size_t lightingBytes, void *shadow, const uint32_t *damage) {
    @autoreleasepool {
        IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
        IPPMetalDevice *device = frame.device;
        IPPMetalTarget *target = image ? (__bridge IPPMetalTarget *)image : device.root;
        if (!flush_canvas(frame)) return false;
        [frame.encoder endEncoding];
        frame.encoder = nil;
        frame.target = target;
        MTLRenderPassDescriptor *pass = [MTLRenderPassDescriptor renderPassDescriptor];
        pass.colorAttachments[0].texture = target.color;
        pass.colorAttachments[0].loadAction = damage ? MTLLoadActionLoad : MTLLoadActionClear;
        pass.colorAttachments[0].storeAction = MTLStoreActionStore;
        pass.colorAttachments[0].clearColor = MTLClearColorMake(
            background[0], background[1], background[2], background[3]);
        if (target.owner) {
            pass.colorAttachments[1].texture = target.owner;
            pass.colorAttachments[1].loadAction = MTLLoadActionClear;
            pass.colorAttachments[1].storeAction = MTLStoreActionStore;
            pass.colorAttachments[1].clearColor = MTLClearColorMake(0, 0, 0, 0);
        }
        pass.depthAttachment.texture = target.depth;
        pass.depthAttachment.loadAction = MTLLoadActionClear;
        pass.depthAttachment.storeAction = MTLStoreActionDontCare;
        pass.depthAttachment.clearDepth = 1;
        frame.encoder = [frame.commands renderCommandEncoderWithDescriptor:pass];
        if (!frame.encoder) return false;
        frame.drawState = 0;
        frame.material = nil;
        frame.depthOnly = false;
        if (!target.owner) [frame.encoder setRenderPipelineState:image ? device.meshImage : device.meshRoot];
        [frame.encoder setDepthStencilState:device.depthState];
        [frame.encoder setFrontFacingWinding:MTLWindingCounterClockwise];
        [frame.encoder setCullMode:MTLCullModeBack];
        if (lighting) [frame.encoder setFragmentBytes:lighting length:lightingBytes atIndex:0];
        [frame.encoder setFragmentTexture:shadow ? (__bridge id<MTLTexture>)shadow : device.emptyDepth atIndex:1];
        if (damage) {
            [frame.encoder setScissorRect:(MTLScissorRect){damage[0], damage[1], damage[2], damage[3]}];
            [frame.encoder setRenderPipelineState:device.canvasClear];
            [frame.encoder setDepthStencilState:device.canvasOverlay];
            [frame.encoder setCullMode:MTLCullModeNone];
            [frame.encoder drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:3];
        }
        return true;
    }
}

bool ipp_metal_canvas_columns(void *handle, const void *mappings, size_t mappingBytes, size_t count,
                             uint32_t width, uint32_t height) {
    @autoreleasepool {
        IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
        IPPMetalDevice *device = frame.device;
        id<MTLComputePipelineState> pipeline = device.canvasColumnsPipeline;
        if (!pipeline || !count || !width || !height || mappingBytes > 4096 || !flush_canvas(frame)) {
            return false;
        }
        for (IPPMetalColumnMap *entry in device.columnCache.reverseObjectEnumerator) {
            if (entry.ready && entry.width == width && entry.height == height
                && entry.mappings.length == mappingBytes
                && memcmp(entry.mappings.bytes, mappings, mappingBytes) == 0) {
                frame.canvasColumns = entry.buffer;
                return true;
            }
        }

        // A moving chart gets the direct shader. Build a column map only
        // once the same view has been requested again.
        NSMutableData *previous = device.previousColumnMappings;
        if (device.previousColumnWidth != width || device.previousColumnHeight != height
            || previous.length != mappingBytes
            || memcmp(previous.bytes, mappings, mappingBytes) != 0) {
            previous.length = mappingBytes;
            memcpy(previous.mutableBytes, mappings, mappingBytes);
            device.previousColumnWidth = width;
            device.previousColumnHeight = height;
            return false;
        }

        const size_t stride = width + 2;
        id<MTLBuffer> columns = [device.gpu newBufferWithLength:CanvasColumnBytes * stride * count
                                                      options:MTLResourceStorageModePrivate];
        if (!columns) return false;
        [frame.encoder endEncoding];
        frame.encoder = nil;
        id<MTLComputeCommandEncoder> compute = [frame.commands computeCommandEncoder];
        if (!compute) return false;

        const uint32_t viewport[2] = {width, height};
        compute.label = @"IPP cylinder columns";
        [compute setComputePipelineState:pipeline];
        [compute setBytes:mappings length:mappingBytes atIndex:0];
        [compute setBuffer:columns offset:0 atIndex:1];
        [compute setBytes:viewport length:sizeof(viewport) atIndex:2];
        [compute dispatchThreads:MTLSizeMake(stride, count, 1)
           threadsPerThreadgroup:MTLSizeMake(pipeline.threadExecutionWidth, 1, 1)];
        [compute endEncoding];

        // Only completed GPU writes may serve future frames. Pending or
        // failed frames cannot publish a partially initialized column map.
        IPPMetalColumnMap *entry = [IPPMetalColumnMap new];
        entry.mappings = [NSData dataWithBytes:mappings length:mappingBytes];
        entry.buffer = columns;
        entry.width = width;
        entry.height = height;
        if (device.columnCache.count == 4) [device.columnCache removeObjectAtIndex:0];
        [device.columnCache addObject:entry];
        [frame.commands addCompletedHandler:^(id<MTLCommandBuffer> command) {
            entry.ready = command.status == MTLCommandBufferStatusCompleted;
        }];
        frame.canvasColumns = columns;
        return true;
    }
}

void ipp_metal_lighting(void *handle, const void *lighting, size_t length) {
    @autoreleasepool {
        IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
        if (flush_canvas(frame)) [frame.encoder setFragmentBytes:lighting length:length atIndex:0];
    }
}

static size_t canvas_buffer_offset(size_t size) {
    return (size + 255) & ~(size_t)255;
}

static bool encode_canvas(IPPMetalFrame *frame, IPPMetalCanvasBatch *batch) {
    IPPMetalDevice *device = frame.device;
    id<MTLRenderCommandEncoder> draw = frame.encoder;
    bool layered = frame.target.owner != nil;
    uint8_t canvasState = batch.specialization ? batch.specialization : (batch.analytic || layered ? 3 : 4);
    IPPMetalCanvasProgram *custom = batch.program;
    if (frame.drawState != canvasState || frame.paint != custom) {
        bool root = frame.target == device.root;
        id<MTLRenderPipelineState> selected;
        if (layered) {
            selected = custom ? custom.layers : device.canvasLayers;
        } else if (batch.specialization == IPPMetalCanvasCylinderPath) {
            selected = root ? device.canvasCylinderPathRoot : device.canvasCylinderPathImage;
        } else if (batch.specialization == IPPMetalCanvasCylinderShape) {
            selected = custom ? (root ? custom.shapeRoot : custom.shapeImage)
                              : (root ? device.canvasShapeRoot : device.canvasShapeImage);
        } else if (batch.specialization == IPPMetalCanvasColumnShape) {
            selected = custom ? (root ? custom.columnRoot : custom.columnImage)
                              : (root ? device.canvasColumnRoot : device.canvasColumnImage);
        } else if (batch.specialization == IPPMetalCanvasColumnPath) {
            selected = root ? device.canvasColumnPathRoot : device.canvasColumnPathImage;
        } else if (batch.specialization == 6) {
            selected = root ? device.canvasPathRoot : device.canvasPathImage;
        } else if (batch.specialization == 7) {
            selected = root ? device.canvasTextureRoot : device.canvasTextureImage;
        } else if (batch.analytic) {
            selected = custom ? (root ? custom.root : custom.image)
                              : (root ? device.canvasRoot : device.canvasImage);
        } else {
            selected = custom ? (root ? custom.affineRoot : custom.affineImage)
                              : (root ? device.canvasAffineRoot : device.canvasAffineImage);
        }
        [draw setRenderPipelineState:selected];
        [draw setCullMode:MTLCullModeNone];
        frame.paint = custom;
        frame.drawState = canvasState;
    }

    [draw setDepthStencilState:batch.depthTest ? device.canvasDepth : device.canvasOverlay];
    const size_t vertexBytes = batch.vertices.length;
    const size_t paintBytes = batch.paints.length;
    const size_t bindingBytes = batch.bindings.length;
    const size_t mappingBytes = batch.mapping.length;
    const size_t analyticBytes = batch.intersections.length;
    const size_t edgeBytes = batch.edges.length;
    const size_t parameterBytes = batch.parameters.length;
    const size_t packedParameterBytes = batch.analytic ? parameterBytes : 0;
    if (vertexBytes > 4096 || paintBytes > 4096 || mappingBytes > 4096
        || analyticBytes > 4096 || edgeBytes > 4096 || packedParameterBytes > 4096) {
        size_t paintsAt = canvas_buffer_offset(vertexBytes);
        size_t bindingsAt = paintsAt + canvas_buffer_offset(paintBytes);
        size_t mappingAt = bindingsAt + canvas_buffer_offset(bindingBytes);
        size_t intersectionAt = mappingAt + canvas_buffer_offset(mappingBytes);
        size_t edgesAt = intersectionAt + canvas_buffer_offset(analyticBytes);
        size_t parametersAt = edgesAt + canvas_buffer_offset(edgeBytes);
        size_t bytes = parametersAt + packedParameterBytes;
        size_t base = 0;
        id<MTLBuffer> data = batch.retainedData;
        if (data) {
            // A recording owns an immutable upload with base offset zero.
        } else if (frame.canvasRecording) {
            data = [device.gpu newBufferWithLength:bytes options:MTLResourceStorageModeShared];
        } else if (batch.analytic) {
            // Each command buffer owns its uploaded batches. Allocate disjoint
            // aligned slices so the GPU can consume them after this call returns.
            base = frame.canvasUploadOffset;
            data = frame.canvasUpload;
            if (!data || bytes > data.length - base) {
                data = [device.gpu newBufferWithLength:MAX(64 * 1024, canvas_buffer_offset(bytes))
                                              options:MTLResourceStorageModeShared];
                frame.canvasUpload = data;
                base = 0;
            }
            frame.canvasUploadOffset = base + canvas_buffer_offset(bytes);
        } else {
            data = [device.gpu newBufferWithLength:bytes options:MTLResourceStorageModeShared];
        }
        if (!data) {
            frame.encodingFailed = true;
            return false;
        }
        const bool upload = batch.retainedData == nil;
        uint8_t *mapped = (uint8_t *)data.contents + base;
        if (vertexBytes) {
            if (upload) memcpy(mapped, batch.vertices.bytes, vertexBytes);
            [draw setVertexBuffer:data offset:base atIndex:0];
        }
        if (upload) memcpy(mapped + paintsAt, batch.paints.bytes, paintBytes);
        if (upload) memcpy(mapped + bindingsAt, batch.bindings.bytes, bindingBytes);
        [draw setFragmentBuffer:data offset:base + paintsAt atIndex:1];
        [draw setFragmentBuffer:data offset:base + bindingsAt atIndex:6];
        if (analyticBytes) {
            if (upload) memcpy(mapped + mappingAt, batch.mapping.bytes, mappingBytes);
            if (upload) memcpy(mapped + intersectionAt, batch.intersections.bytes, analyticBytes);
            [draw setFragmentBuffer:data offset:base + mappingAt atIndex:8];
            [draw setFragmentBuffer:data offset:base + intersectionAt atIndex:5];
            [draw setVertexBuffer:data offset:base + mappingAt atIndex:2];
            [draw setVertexBuffer:data offset:base + intersectionAt atIndex:1];
        }
        if (edgeBytes) {
            if (upload) memcpy(mapped + edgesAt, batch.edges.bytes, edgeBytes);
            [draw setFragmentBuffer:data offset:base + edgesAt atIndex:7];
        }
        if (packedParameterBytes) {
            if (upload) memcpy(mapped + parametersAt, batch.parameters.bytes, packedParameterBytes);
            [draw setFragmentBuffer:data offset:base + parametersAt atIndex:3];
        }
        if (frame.canvasRecording && upload) {
            batch.retainedData = data;
            // Account for both the upload and the staging bytes retained here.
            frame.canvasRecordedBytes += 2 * bytes + 1024;
        }
    } else {
        if (vertexBytes) [draw setVertexBytes:batch.vertices.bytes length:vertexBytes atIndex:0];
        [draw setFragmentBytes:batch.paints.bytes length:paintBytes atIndex:1];
        [draw setFragmentBytes:batch.bindings.bytes length:bindingBytes atIndex:6];
        if (analyticBytes) {
            [draw setFragmentBytes:batch.mapping.bytes length:mappingBytes atIndex:8];
            [draw setFragmentBytes:batch.intersections.bytes length:analyticBytes atIndex:5];
            [draw setVertexBytes:batch.mapping.bytes length:mappingBytes atIndex:2];
            [draw setVertexBytes:batch.intersections.bytes length:analyticBytes atIndex:1];
        }
        if (edgeBytes) [draw setFragmentBytes:batch.edges.bytes length:edgeBytes atIndex:7];
        if (packedParameterBytes) [draw setFragmentBytes:batch.parameters.bytes length:packedParameterBytes atIndex:3];
    }
    const float unused[4] = {0};
    if (!edgeBytes && batch.analytic) [draw setFragmentBytes:unused length:sizeof(unused) atIndex:7];
    if (batch.analytic) [draw setFragmentBuffer:frame.canvasColumns ?: device.emptyColumns offset:0 atIndex:9];
    if (!packedParameterBytes) {
        [draw setFragmentBytes:parameterBytes ? batch.parameters.bytes : unused
                         length:parameterBytes ? parameterBytes : sizeof(unused) atIndex:3];
    }

    [draw setFragmentTexture:batch.color atIndex:0];
    [draw setFragmentTexture:batch.mask atIndex:2];
    if (batch.path) {
        [draw setFragmentBuffer:batch.path offset:0 atIndex:2];
    } else {
        [draw setFragmentBytes:unused length:sizeof(unused) atIndex:2];
    }

    [draw drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0
             vertexCount:(batch.analytic ? 6 * batch.count : vertexBytes / sizeof(IPPMetalCanvasVertex))];
    frame.mergedCanvasDraws += batch.count - 1;
    return true;
}

static bool flush_canvas(IPPMetalFrame *frame) {
    if (frame.encodingFailed) return false;
    IPPMetalCanvasBatch *batch = frame.canvasBatch;
    if (batch.count == 0) return true;
    if (!encode_canvas(frame, batch)) return false;

    if (frame.canvasRecording) {
        if (!batch.retainedData) {
            frame.canvasRecordedBytes += batch.vertices.length + batch.paints.length
                + batch.bindings.length + batch.mapping.length + batch.intersections.length
                + batch.edges.length + batch.parameters.length + 1024;
        }
        [frame.canvasRecording addObject:batch];
        frame.canvasBatch = nil;
    } else {
        [batch reset];
    }
    return true;
}

bool ipp_metal_presentation_extent(void *handle, uint32_t *extent) {
    IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
    if (!frame.target || frame.target != frame.device.root) return false;
    extent[0] = (uint32_t)frame.target.color.width;
    extent[1] = (uint32_t)frame.target.color.height;
    return true;
}

bool ipp_metal_begin_canvas_record(void *handle) {
    @autoreleasepool {
        IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
        if (frame.canvasRecording || !flush_canvas(frame)) return false;
        frame.canvasRecording = [NSMutableArray array];
        frame.canvasRecordedBytes = 0;
        return true;
    }
}

void *ipp_metal_end_canvas_record(void *handle, size_t *bytes) {
    @autoreleasepool {
        IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
        bool encoded = flush_canvas(frame);
        NSArray<IPPMetalCanvasBatch *> *record = frame.canvasRecording;
        frame.canvasRecording = nil;
        *bytes = frame.canvasRecordedBytes;
        return encoded ? (__bridge_retained void *)record : NULL;
    }
}

bool ipp_metal_replay_canvas(void *handle, void *record) {
    @autoreleasepool {
        IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
        if (!flush_canvas(frame)) return false;
        for (IPPMetalCanvasBatch *batch in (__bridge NSArray<IPPMetalCanvasBatch *> *)record) {
            if (!encode_canvas(frame, batch)) return false;
        }
        return true;
    }
}

bool ipp_metal_canvas(void *handle, const void *vertices, size_t vertexBytes, size_t count,
                      const void *paint, size_t paintBytes, void *bitmap, void *image,
                      void *path, uint32_t pathStart, void *program, const void *parameters,
                      size_t parameterBytes, const void *mapping, size_t mappingBytes, bool usesColumns, bool isCylinder,
                      const void *analytic, size_t analyticBytes,
                      const void *edges, size_t edgeBytes, bool depthTest, uint32_t owner, uint32_t selectedLayer) {
    @autoreleasepool {
        IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
        bool usesIntersection = analytic != NULL;
        if (frame.encodingFailed || usesIntersection == (count != 0)
            || (usesIntersection && frame.target.owner)
            || vertexBytes != count * sizeof(IPPMetalCanvasVertex)) {
            return false;
        }

        IPPMetalDevice *device = frame.device;
        IPPMetalTarget *sampled = (__bridge IPPMetalTarget *)image;
        id<MTLTexture> color = (__bridge id<MTLTexture>)bitmap ?: sampled.color ?: device.white;
        id<MTLTexture> mask = sampled.owner ?: device.emptyOwner;
        id<MTLBuffer> pathBuffer = (__bridge id<MTLBuffer>)path;
        IPPMetalCanvasProgram *custom = (__bridge IPPMetalCanvasProgram *)program;
        uint8_t specialization = 0;
        if (!custom && !frame.target.owner) {
            if (usesIntersection && pathBuffer) specialization = 6;
            if (!usesIntersection && (image || bitmap)) specialization = 7;
        }
        if (isCylinder && specialization == 6) specialization = usesColumns ? IPPMetalCanvasColumnPath : IPPMetalCanvasCylinderPath;
        if (isCylinder && !pathBuffer && !image && !bitmap) {
            specialization = usesColumns ? IPPMetalCanvasColumnShape : IPPMetalCanvasCylinderShape;
        }
        if (!frame.canvasBatch) frame.canvasBatch = [IPPMetalCanvasBatch new];
        IPPMetalCanvasBatch *batch = frame.canvasBatch;
        if (batch.count != 0 &&
            (batch.count == 64 || batch.vertices.length + vertexBytes > 128 * 1024
             || batch.specialization != specialization || batch.color != color || batch.mask != mask
             || (batch.path && pathBuffer && batch.path != pathBuffer)
             || batch.program != custom || batch.depthTest != depthTest || batch.analytic != usesIntersection
             || (usesIntersection && memcmp(batch.mapping.bytes, mapping, mappingBytes) != 0)
             || (!usesIntersection && (batch.parameters.length != parameterBytes
                 || (parameterBytes && memcmp(batch.parameters.bytes, parameters, parameterBytes) != 0))))) {
            if (!flush_canvas(frame)) return false;
            if (!frame.canvasBatch) frame.canvasBatch = [IPPMetalCanvasBatch new];
            batch = frame.canvasBatch;
        }
        if (batch.count == 0) {
            batch.color = color;
            batch.mask = mask;
            batch.program = custom;
            batch.analytic = usesIntersection;
            batch.specialization = specialization;
            batch.depthTest = depthTest;
            if (usesIntersection) [batch.mapping appendBytes:mapping length:mappingBytes];
            else if (parameterBytes) [batch.parameters appendBytes:parameters length:parameterBytes];
        }
        if (pathBuffer && !batch.path) batch.path = pathBuffer;

        if (count) {
            size_t offset = batch.vertices.length;
            [batch.vertices appendBytes:vertices length:vertexBytes];
            IPPMetalCanvasVertex *staged = (IPPMetalCanvasVertex *)((uint8_t *)batch.vertices.mutableBytes + offset);
            for (size_t i = 0; i < count; ++i) staged[i].slot = batch.count;
        }
        [batch.paints appendBytes:paint length:paintBytes];
        uint32_t firstEdge = usesIntersection ? (uint32_t)(batch.edges.length / 16) + 1 : 0;
        if (usesIntersection) [batch.intersections appendBytes:analytic length:analyticBytes];
        uint32_t parameterOffset = 0;
        if (usesIntersection && parameterBytes) {
            size_t length = batch.parameters.length;
            if (length < parameterBytes
                || memcmp((const uint8_t *)batch.parameters.bytes + length - parameterBytes,
                          parameters, parameterBytes) != 0) {
                [batch.parameters appendBytes:parameters length:parameterBytes];
            }
            parameterOffset = (uint32_t)((batch.parameters.length - parameterBytes) / 16);
        }
        // A 64-record batch fits both the clip index and parameter offset
        // in 16 bits. Keep the original 16-byte binding stride.
        const uint32_t binding[4] = {owner, selectedLayer, pathStart, firstEdge | (parameterOffset << 16)};
        [batch.bindings appendBytes:binding length:sizeof(binding)];
        if (edgeBytes) [batch.edges appendBytes:edges length:edgeBytes];
        ++batch.count;
        return true;
    }
}

bool ipp_metal_draw(void *handle, void *vertices, void *indices, size_t indexCount,
                    void *pose, void *color, const void *uniforms, size_t uniformBytes, size_t instances,
                    const void *joints, size_t jointBytes, uint8_t blend, void *program,
                    const void *parameters, size_t parameterBytes, void *const *textures, size_t textureCount) {
    @autoreleasepool {
        IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
        if (!flush_canvas(frame)) return false;
        id<MTLRenderCommandEncoder> draw = frame.encoder;
        IPPMetalMaterialProgram *custom = (__bridge IPPMetalMaterialProgram *)program;
        if (frame.depthOnly && frame.material != custom) {
            [draw setRenderPipelineState:custom ? custom.shadow : frame.device.shadowPipeline];
            frame.material = custom;
        }
        if (!frame.depthOnly && (frame.drawState != blend || frame.material != custom)) {
            IPPMetalDevice *device = frame.device;
            bool root = frame.target == device.root;
            id<MTLRenderPipelineState> selected = root ? device.meshRoot : device.meshImage;
            if (blend == 1) selected = root ? device.alphaRoot : device.alphaImage;
            if (blend == 2) selected = root ? device.additiveRoot : device.additiveImage;
            if (custom) {
                selected = blend
                    ? (root ? custom.alphaRoot : custom.alphaImage)
                    : (root ? custom.root : custom.image);
            }
            frame.material = custom;
            [draw setRenderPipelineState:selected];
            [draw setDepthStencilState:blend ? device.canvasDepth : device.depthState];
            [draw setCullMode:MTLCullModeBack];
            frame.drawState = blend;
        }
        [draw setVertexBuffer:(__bridge id<MTLBuffer>)vertices offset:0 atIndex:0];
        if (uniformBytes > 4096) {
            id<MTLBuffer> instancesBuffer = [frame.device.gpu newBufferWithBytes:uniforms
                length:uniformBytes options:MTLResourceStorageModeShared];
            if (!instancesBuffer) return false;
            [draw setVertexBuffer:instancesBuffer offset:0 atIndex:1];
        } else {
            [draw setVertexBytes:uniforms length:uniformBytes atIndex:1];
        }
        [draw setVertexBuffer:(__bridge id<MTLBuffer>)(pose ?: vertices) offset:0 atIndex:2];
        if (jointBytes > 4096) {
            id<MTLBuffer> palette = [frame.commands.device newBufferWithBytes:joints
                length:jointBytes options:MTLResourceStorageModeShared];
            if (!palette) return false;
            [draw setVertexBuffer:palette offset:0 atIndex:3];
        } else {
            [draw setVertexBytes:joints length:jointBytes atIndex:3];
        }
        if (!frame.depthOnly)
            [draw setFragmentTexture:color ? (__bridge id<MTLTexture>)color : frame.device.white atIndex:0];
        if (custom) {
            if (frame.depthOnly) [draw setFragmentTexture:frame.device.emptyDepth atIndex:1];
            const uint32_t zero[4] = {0};
            const void *data = parameterBytes ? parameters : zero;
            size_t length = parameterBytes ? parameterBytes : sizeof(zero);
            [draw setVertexBytes:data length:length atIndex:4];
            [draw setFragmentBytes:data length:length atIndex:4];
            for (size_t i = 0; i < textureCount; i++) {
                id<MTLTexture> entry = (__bridge id<MTLTexture>)textures[i];
                [draw setVertexTexture:entry atIndex:i + 2];
                [draw setFragmentTexture:entry atIndex:i + 2];
            }
        }
        if (indices) {
            [draw drawIndexedPrimitives:MTLPrimitiveTypeTriangle indexCount:indexCount
                             indexType:MTLIndexTypeUInt16 indexBuffer:(__bridge id<MTLBuffer>)indices
                     indexBufferOffset:0 instanceCount:instances];
        } else {
            [draw drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:indexCount instanceCount:instances];
        }
        return true;
    }
}

#ifdef IPP_METAL_INSTRUMENTATION
bool ipp_metal_gpu_timing_available(void) {
    if (@available(macOS 10.15, *)) return true;
    return false;
}
#endif

// Metal retains the contents of all three command buffers. Ordinary frames may
// return after submission; capture and requested GPU durations wait on this queue.
static bool drain_commands(IPPMetalDevice *device, char *message, size_t capacity) {
    while (device.submitted.count > 0) {
        id<MTLCommandBuffer> oldest = device.submitted.firstObject;
        if (device.submitted.count >= 3) [oldest waitUntilCompleted];
        if (oldest.status < MTLCommandBufferStatusCompleted) break;
        [device.submitted removeObjectAtIndex:0];
        if (oldest.status == MTLCommandBufferStatusError) {
            describe(message, capacity, oldest.error.localizedDescription);
            return false;
        }
    }
    return true;
}

bool ipp_metal_finish(void *handle, uint8_t *pixels, size_t length, char *message, size_t capacity,
                      double *gpuSeconds, uint32_t *mergedCanvasDraws) {
    @autoreleasepool {
        IPPMetalFrame *frame = (__bridge IPPMetalFrame *)handle;
        if (!flush_canvas(frame)) {
            describe(message, capacity, @"Could not encode Metal Canvas batch");
            return false;
        }
        *mergedCanvasDraws = frame.mergedCanvasDraws;
        [frame.encoder endEncoding];
        frame.encoder = nil;
        const size_t row = frame.color.width * 4;
        const size_t height = frame.color.height;
        const size_t stride = (row + 255) & ~(size_t)255;
        id<MTLBuffer> capture = nil;
        if (pixels) {
            if (length != row * height) {
                describe(message, capacity, @"Invalid Metal readback extent");
                return false;
            }
            capture = [frame.commands.device newBufferWithLength:stride * height
                options:MTLResourceStorageModeShared];
            id<MTLBlitCommandEncoder> blit = capture ? [frame.commands blitCommandEncoder] : nil;
            if (!blit) {
                describe(message, capacity, @"Could not allocate Metal readback");
                return false;
            }
            [blit copyFromTexture:frame.color sourceSlice:0 sourceLevel:0
                     sourceOrigin:MTLOriginMake(0, 0, 0)
                       sourceSize:MTLSizeMake(frame.color.width, height, 1)
                         toBuffer:capture destinationOffset:0
               destinationBytesPerRow:stride destinationBytesPerImage:stride * height];
            [blit endEncoding];
        }
        if (!drain_commands(frame.device, message, capacity)) return false;
        [frame.commands commit];
        if (!capture && !gpuSeconds) {
            [frame.device.submitted addObject:frame.commands];
            return true;
        }
        [frame.commands waitUntilCompleted];
        if (!drain_commands(frame.device, message, capacity)) return false;
        if (frame.commands.status != MTLCommandBufferStatusCompleted) {
            describe(message, capacity, frame.commands.error.localizedDescription);
            return false;
        }
#ifdef IPP_METAL_INSTRUMENTATION
        if (gpuSeconds && !capture) {
            if (@available(macOS 10.15, *)) {
                double start = frame.commands.GPUStartTime;
                double end = frame.commands.GPUEndTime;
                if (start > 0 && end > start) *gpuSeconds = end - start;
            }
        }
#else
        (void)gpuSeconds;
#endif
        if (capture) {
            const uint8_t *source = capture.contents;
            for (size_t y = 0; y < height; ++y) memcpy(pixels + y * row, source + y * stride, row);
        }
        return true;
    }
}
